use std::convert::Infallible;
use std::io::{Read, Write};
use std::ops::Deref;
use std::{fmt, io, time};

use crossbeam_channel as chan;
use radicle::node::NodeId;

use crate::runtime::Handle;
use crate::wire::StreamId;

pub struct Tunnel {
    receiver: ChannelReader,
    sender: ChannelFlushWriter,
}

impl Tunnel {
    pub fn new(handle: Handle, channels: Channels, remote: NodeId) -> Self {
        Self {
            receiver: channels.receiver,
            sender: ChannelFlushWriter {
                writer: channels.sender,
                handle,
                remote,
            },
        }
    }

    pub fn split(&mut self) -> (&mut ChannelReader, &mut ChannelFlushWriter) {
        (&mut self.receiver, &mut self.sender)
    }
}

impl radicle::fetch::transport::ConnectionStream for Tunnel {
    type Read = ChannelReader;
    type Write = ChannelFlushWriter;
    type Error = Infallible;

    fn open(&mut self) -> Result<(&mut Self::Read, &mut Self::Write), Self::Error> {
        Ok((&mut self.receiver, &mut self.sender))
    }
}

/// Data that can be sent and received on worker channels.
pub enum ChannelEvent<T = Vec<u8>> {
    /// Git protocol data.
    Data(T),
    /// A request to close the channel.
    Close,
    /// A signal that the git protocol has ended, eg. when the remote fetch closes the
    /// connection.
    Eof,
}

impl<T> From<T> for ChannelEvent<T> {
    fn from(value: T) -> Self {
        Self::Data(value)
    }
}

impl<T> fmt::Debug for ChannelEvent<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data(_) => write!(f, "ChannelEvent::Data(..)"),
            Self::Close => write!(f, "ChannelEvent::Close"),
            Self::Eof => write!(f, "ChannelEvent::Eof"),
        }
    }
}

/// Worker channels for communicating through the git stream with the remote.
pub struct Channels<T = Vec<u8>> {
    sender: ChannelWriter<T>,
    receiver: ChannelReader<T>,
}

impl<T: AsRef<[u8]>> Channels<T> {
    pub fn new(
        sender: chan::Sender<ChannelEvent<T>>,
        receiver: chan::Receiver<ChannelEvent<T>>,
        stream: StreamId,
        timeout: time::Duration,
    ) -> Self {
        let sender = ChannelWriter {
            sender,
            stream,
            timeout,
        };
        let receiver = ChannelReader::new(receiver, stream, timeout);

        Self { sender, receiver }
    }

    pub fn pair(
        stream: StreamId,
        timeout: time::Duration,
    ) -> io::Result<(Channels<T>, Channels<T>)> {
        let (l_send, r_recv) = chan::unbounded::<ChannelEvent<T>>();
        let (r_send, l_recv) = chan::unbounded::<ChannelEvent<T>>();

        let l = Channels::new(l_send, l_recv, stream, timeout);
        let r = Channels::new(r_send, r_recv, stream, timeout);

        Ok((l, r))
    }

    pub fn try_iter(&self) -> impl Iterator<Item = ChannelEvent<T>> + '_ {
        self.receiver.try_iter()
    }

    pub fn split(&mut self) -> (&mut ChannelWriter<T>, &mut ChannelReader<T>) {
        (&mut self.sender, &mut self.receiver)
    }

    pub fn send(&self, event: ChannelEvent<T>) -> io::Result<()> {
        self.sender.send(event)
    }

    pub fn close(self) -> Result<(), chan::SendError<ChannelEvent<T>>> {
        self.sender.close()
    }
}

/// Wraps a [`chan::Receiver`] and provides it with [`io::Read`].
#[derive(Clone)]
pub struct ChannelReader<T = Vec<u8>> {
    buffer: io::Cursor<Vec<u8>>,
    receiver: chan::Receiver<ChannelEvent<T>>,
    stream: StreamId,
    timeout: time::Duration,
}

impl<T> Deref for ChannelReader<T> {
    type Target = chan::Receiver<ChannelEvent<T>>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl<T: AsRef<[u8]>> ChannelReader<T> {
    pub fn new(
        receiver: chan::Receiver<ChannelEvent<T>>,
        stream: StreamId,
        timeout: time::Duration,
    ) -> Self {
        Self {
            buffer: io::Cursor::new(Vec::new()),
            receiver,
            stream,
            timeout,
        }
    }

    pub fn pipe<W: io::Write>(&mut self, mut writer: W) -> io::Result<()> {
        loop {
            match self.receiver.recv_timeout(self.timeout) {
                Ok(ChannelEvent::Data(data)) => writer.write_all(data.as_ref())?,
                Ok(ChannelEvent::Eof) => return Ok(()),
                Ok(ChannelEvent::Close) => return Err(io::ErrorKind::ConnectionReset.into()),
                Err(chan::RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "error reading from stream: channel timed out",
                    ));
                }
                Err(chan::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "error reading from stream: channel is disconnected",
                    ));
                }
            }
        }
    }
}

impl Read for ChannelReader<Vec<u8>> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.buffer.read(buf)?;
        if read > 0 {
            println!("Read Data: {:?}", std::str::from_utf8(buf));
            return Ok(read);
        }

        match self.receiver.recv_timeout(self.timeout) {
            Ok(ChannelEvent::Data(data)) => {
                println!(
                    "Read Data stream={}: {:?}",
                    self.stream,
                    std::str::from_utf8(&data)
                );
                self.buffer = io::Cursor::new(data);
                self.buffer.read(buf)
            }
            Ok(ChannelEvent::Eof) => Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(ChannelEvent::Close) => Err(io::ErrorKind::ConnectionReset.into()),

            Err(chan::RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "error reading from stream: channel timed out",
            )),
            Err(chan::RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "error reading from stream: channel is disconnected",
            )),
        }
    }
}

/// Wraps a [`chan::Sender`] and provides it with [`io::Write`].
#[derive(Clone)]
pub struct ChannelWriter<T = Vec<u8>> {
    sender: chan::Sender<ChannelEvent<T>>,
    stream: StreamId,
    timeout: time::Duration,
}

pub struct ChannelFlushWriter<T = Vec<u8>> {
    writer: ChannelWriter<T>,
    handle: Handle,
    remote: NodeId,
}

impl Write for ChannelFlushWriter<Vec<u8>> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        println!(
            "Write Data stream={} node={}: {:?}",
            self.writer.stream,
            self.remote,
            std::str::from_utf8(buf)
        );
        let n = buf.len();
        self.writer.send(buf.to_vec())?;
        self.flush()?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.handle.flush(self.remote, self.writer.stream)
    }
}

impl<T: AsRef<[u8]>> ChannelWriter<T> {
    pub fn send(&self, event: impl Into<ChannelEvent<T>>) -> io::Result<()> {
        match self.sender.send_timeout(event.into(), self.timeout) {
            Ok(()) => Ok(()),
            Err(chan::SendTimeoutError::Timeout(_)) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "error writing to stream: channel timed out",
            )),
            Err(chan::SendTimeoutError::Disconnected(_)) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "error writing to stream: channel is disconnected",
            )),
        }
    }

    /// Since the git protocol is tunneled over an existing connection, we can't signal the end of
    /// the protocol via the usual means, which is to close the connection. Git also doesn't have
    /// any special message we can send to signal the end of the protocol.
    ///
    /// Hence, there's no other way for the server to know that we're done sending requests
    /// than to send a special message outside the git protocol. This message can then be processed
    /// by the remote worker to end the protocol. We use the special "eof" control message for this.
    pub fn eof(&self) -> Result<(), chan::SendError<ChannelEvent<T>>> {
        self.sender.send(ChannelEvent::Eof)
    }

    /// Permanently close this stream.
    pub fn close(self) -> Result<(), chan::SendError<ChannelEvent<T>>> {
        self.sender.send(ChannelEvent::Close)
    }
}
