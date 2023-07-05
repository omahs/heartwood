use std::io::Write;
use std::io::{self, Read};
use std::process::{Command, ExitStatus, Stdio};
use std::str::FromStr;
use std::thread;

use gix_packetline::{self as packetline, PacketLineRef};
use once_cell::sync::Lazy;
use radicle::{storage::ReadStorage, Storage};
use versions::Version;

#[derive(Debug, PartialEq, Eq)]
pub struct Header {
    pub path: String,
    pub host: Option<(String, Option<u16>)>,
    pub extra: Vec<(String, Option<String>)>,
}

impl FromStr for Header {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s
            .strip_prefix("git-upload-pack ")
            .ok_or("unsupported service")?
            .split_terminator('\0');

        let path = parts.next().ok_or("missing path").and_then(|path| {
            if path.is_empty() {
                Err("empty path")
            } else {
                Ok(path.to_owned())
            }
        })?;
        let host = match parts.next() {
            None | Some("") => None,
            Some(host) => match host.strip_prefix("host=") {
                None => return Err("invalid host"),
                Some(host) => match host.split_once(':') {
                    None => Some((host.to_owned(), None)),
                    Some((host, port)) => {
                        let port = port.parse::<u16>().or(Err("invalid port"))?;
                        Some((host.to_owned(), Some(port)))
                    }
                },
            },
        };
        let extra = parts
            .skip_while(|part| part.is_empty())
            .map(|part| match part.split_once('=') {
                None => (part.to_owned(), None),
                Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            })
            .collect();

        Ok(Self { path, host, extra })
    }
}

pub fn header<R>(mut recv: R) -> io::Result<(Header, R)>
where
    R: io::Read + Send,
{
    log::debug!(target: "worker", "upload-pack waiting for header");
    let mut pktline = packetline::StreamingPeekableIter::new(recv, &[]);
    let pkt = pktline
        .read_line()
        .ok_or_else(|| invalid_data("missing header"))?
        .map_err(invalid_data)?
        .map_err(invalid_data)?;
    let hdr = match pkt {
        PacketLineRef::Data(data) => std::str::from_utf8(data)
            .map_err(invalid_data)?
            .parse()
            .map_err(invalid_data),
        _ => Err(invalid_data("not a header packet")),
    }?;
    recv = pktline.into_inner();

    Ok((hdr, recv))
}

pub fn upload_pack<R, W>(
    name: String,
    storage: &Storage,
    header: &Header,
    mut recv: R,
    mut send: W,
) -> io::Result<ExitStatus>
where
    R: io::Read + Send,
    W: io::Write + Send,
{
    // let mut recv = io::BufReader::new(recv);
    // let header: Header = match recv.fill_buf()?.first() {
    //     // legacy clients don't send a proper pktline header :(
    //     Some(b'g') => {
    //         let mut buf = String::with_capacity(256);
    //         recv.read_line(&mut buf)?;
    //         buf.parse().map_err(invalid_data)?
    //     }
    //     Some(_) => {
    //         let mut pktline = packetline::StreamingPeekableIter::new(recv, &[]);
    //         let pkt = pktline
    //             .read_line()
    //             .ok_or_else(|| invalid_data("missing header"))?
    //             .map_err(invalid_data)?
    //             .map_err(invalid_data)?;
    //         let hdr = match pkt {
    //             PacketLineRef::Data(data) => std::str::from_utf8(data)
    //                 .map_err(invalid_data)?
    //                 .parse()
    //                 .map_err(invalid_data),
    //             _ => Err(invalid_data("not a header packet")),
    //         }?;
    //         recv = pktline.into_inner();

    //         hdr
    //     }
    //     None => {
    //         return Err(io::Error::new(
    //             io::ErrorKind::UnexpectedEof,
    //             "expected header",
    //         ))
    //     }
    // };

    log::debug!(
        target: "worker",
        "upload-pack received header path={:?}, host={:?}",
        header.path,
        header.host
    );

    let namespace = header
        .path
        .strip_prefix("rad:")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| header.path.clone());
    let protocol_version = header
        .extra
        .iter()
        .find_map(|kv| match kv {
            (ref k, Some(v)) if k == "version" => {
                let version = match v.as_str() {
                    "2" => 2,
                    "1" => 1,
                    _ => 0,
                };
                Some(version)
            }
            _ => None,
        })
        .unwrap_or(0);

    advertise_capabilities(&mut send)?;

    let git_dir = {
        let rid = namespace
            .strip_prefix('/')
            .unwrap_or(&namespace)
            .parse()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        let repo = storage
            .repository(rid)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        repo.backend.path().to_path_buf()
    };

    let mut child = {
        let mut cmd = Command::new("git");
        cmd.current_dir(git_dir)
            .env_clear()
            .envs(std::env::vars().filter(|(key, _)| key == "PATH" || key.starts_with("GIT_TRACE")))
            .env("GIT_PROTOCOL", format!("version={}", protocol_version))
            .args([
                "-c",
                "uploadpack.allowanysha1inwant=true",
                "-c",
                "uploadpack.allowrefinwant=true",
                "-c",
                "lsrefs.unborn=ignore",
                "upload-pack",
                "--strict",
                // TODO: maybe we don't use this and figure out how to keep it persisted
                // "--stateless-rpc",
                ".",
            ])
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit());

        cmd.spawn()?
    };

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = io::BufReader::new(child.stdout.take().unwrap());

    thread::scope(|s| {
        let h1 = thread::Builder::new()
            .name(name.clone())
            .spawn_scoped(s, || {
                // if let Err(err) = io::copy(&mut stdout, &mut send) {
                //     log::debug!(target: "worker", "failed to copy upload pack stdout to channel: {err}");
                // }
                let mut buffer = [0; u16::MAX as usize + 1];
                loop {
                    match stdout.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            println!("Write from upload-pack");
                            send.write_all(&buffer[..n]).unwrap();

                            if let Err(e) = send.flush() {
                                log::error!(target: "worker", "Worker channel disconnected; aborting: {e}");
                                break;
                                // return Err(e);
                            }
                        }
                        Err(e) => {
                            // if e.kind() == io::ErrorKind::UnexpectedEof {
                            //     log::debug!(target: "worker", "Daemon closed the git connection for {rid}");
                            //     break;
                            // }
                            // return Err(e);
                            log::debug!(target: "worker", "Daemon closed the git connection: {e}");
                            break;
                        }
                    }
                }
            })?;

        let h2 = thread::Builder::new()
            .name(name.clone())
            .spawn_scoped(s, || {
                // if let Err(err) = io::copy(&mut recv, &mut stdin) {
                //     log::debug!(target: "worker", "failed to copy upload pack channel to stdin: {err}");
                // }
                let mut buffer = [0; u16::MAX as usize + 1];
                loop {
                    match recv.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Err(e) = stdin.write_all(&buffer[..n]) {
                                log::warn!(target: "worker", "upload-pack error: {e}");
                                break;
                            }
                            // TODO: this won't actually work
                            // if &buffer[..n] == b"0000" {
                            //     if let Err(e) = stdin.flush() {
                            //         log::warn!(target: "worker", "upload-pack error: {e}");
                            //     }
                            //     break;
                            // }
                        }
                        Err(e) => {
                            if e.kind() == io::ErrorKind::UnexpectedEof {
                                log::debug!(target: "worker", "Daemon closed the git connection");
                                break;
                            }
                            if e.kind() == io::ErrorKind::BrokenPipe {
                                log::error!(target: "worker", "upload-pack error: {e}");
                                break;
                            }
                            // return Err(e);
                        }
                    }
                }
            })?;
        log::debug!("JOINING H1");
        h1.join().unwrap();
        log::debug!("JOINING H2");
        h2.join().unwrap();
        log::debug!("DONE!");
        Ok::<_, io::Error>(())
    })?;

    let status = child.wait()?;
    Ok(status)
}

fn advertise_capabilities<W>(mut send: W) -> io::Result<()>
where
    W: io::Write,
{
    // Thou shallt not upgrade your `git` installation while a link instance is
    // running!
    static GIT_VERSION: Lazy<Version> = Lazy::new(|| git_version().unwrap());
    static AGENT: Lazy<Vec<u8>> = Lazy::new(|| format!("agent=git/{}", *GIT_VERSION).into_bytes());
    static CAPABILITIES: Lazy<[&[u8]; 4]> = Lazy::new(|| {
        [
            b"version 2",
            AGENT.as_slice(),
            b"object-format=sha1",
            b"fetch=ref-in-want",
        ]
    });

    log::debug!(target: "worker", "upload-pack advertising capabilities");
    for cap in *CAPABILITIES {
        packetline::encode::text_to_write(cap, &mut send)?;
    }
    packetline::encode::flush_to_write(&mut send)?;

    Ok(())
}

fn git_version() -> io::Result<Version> {
    let out = std::process::Command::new("git")
        .arg("--version")
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "failed to read `git` version",
        ));
    }

    // parse: git version 2.30.1 <other optional tokens>
    out.stdout
        .split(|x| x == &b' ')
        .nth(2)
        .and_then(|s| {
            let s = std::str::from_utf8(s).ok()?;
            Version::new(s.trim())
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to parse `git` version"))
}

fn invalid_data<E>(inner: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Sync + Send>>,
{
    io::Error::new(io::ErrorKind::InvalidData, inner)
}
