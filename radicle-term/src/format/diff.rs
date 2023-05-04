use std::path::PathBuf;

use radicle_surf::diff::{DiffContent, FileDiff, Hunk, Line, Modification};

use crate::format;

pub struct FilePath {
    path: PathBuf,
}

impl From<PathBuf> for FilePath {
    fn from(path: PathBuf) -> FilePath {
        Self { path }
    }
}

impl ToString for FilePath {
    fn to_string(&self) -> String {
        self.path.as_path().to_str().unwrap().to_string()
    }
}

pub fn file_header(file: &FileDiff) -> anyhow::Result<[crate::Line; 3]> {
    let row = match file {
        FileDiff::Added(added) => {
            let path = FilePath::from(added.path.clone()).to_string();
            let header = format!("{path} (added)");
            [
                format::default(String::new()).into(),
                format::default(String::new()).into(),
                format::bold(header).into(),
            ]
        }
        FileDiff::Deleted(deleted) => {
            let path = FilePath::from(deleted.path.clone()).to_string();
            let header = format!("{path} (deleted)");
            [
                format::default(String::new()).into(),
                format::default(String::new()).into(),
                format::bold(header).into(),
            ]
        }
        FileDiff::Modified(modified) => {
            let header = FilePath::from(modified.path.clone());
            [
                format::default(String::new()).into(),
                format::default(String::new()).into(),
                format::bold(header.to_string()).into(),
            ]
        }
        FileDiff::Moved(moved) => {
            let old_path = FilePath::from(moved.old_path.clone()).to_string();
            let new_path = FilePath::from(moved.new_path.clone()).to_string();
            let header = format!("{old_path} -> {new_path} (moved)");
            [
                format::default(String::new()).into(),
                format::default(String::new()).into(),
                format::bold(header).into(),
            ]
        }
        FileDiff::Copied(copied) => {
            let old_path = FilePath::from(copied.old_path.clone()).to_string();
            let new_path = FilePath::from(copied.new_path.clone()).to_string();
            let header = format!("{old_path} -> {new_path} (copied)");
            [
                format::default(String::new()).into(),
                format::default(String::new()).into(),
                format::bold(header).into(),
            ]
        }
    };

    Ok(row)
}

pub fn file_rows(file: &FileDiff) -> anyhow::Result<Vec<[crate::Line; 3]>> {
    let mut rows = vec![];

    match file {
        FileDiff::Added(added) => {
            for row in self::content(&added.diff)? {
                rows.push(row);
            }
        }
        FileDiff::Deleted(deleted) => {
            for row in self::content(&deleted.diff)? {
                rows.push(row);
            }
        }
        FileDiff::Modified(modified) => {
            for row in self::content(&modified.diff)? {
                rows.push(row);
            }
        }
        _ => {}
    }

    Ok(rows)
}

fn hunk(hunk: &Hunk<Modification>) -> anyhow::Result<Vec<[crate::Line; 3]>> {
    let header = self::line_to_string(&hunk.header)?;
    let mut rows = vec![];

    rows.push([
        format::default(String::new()).into(),
        format::default(String::new()).into(),
        format::faint(format!("{}", header)).into(),
    ]);

    for line in &hunk.lines {
        rows.push(self::modification(line)?);
    }

    Ok(rows)
}

fn content(content: &DiffContent) -> anyhow::Result<Vec<[crate::Line; 3]>> {
    let mut rows = vec![];
    match content {
        DiffContent::Plain { hunks, eof: _ } => {
            for hunk in hunks.iter() {
                rows.append(&mut self::hunk(hunk)?);
            }
        }
        DiffContent::Binary => rows.push(self::message("Cannot display binary file")?),
        _ => {}
    }

    Ok(rows)
}

fn modification(line: &Modification) -> anyhow::Result<[crate::Line; 3]> {
    let row = match line {
        Modification::Addition(addition) => {
            let content = line_to_string(&addition.line)?;
            [
                format::default(String::new()).into(),
                format::positive(addition.line_no).into(),
                format::positive(format!("+ {content}")).into(),
            ]
        }
        Modification::Deletion(deletion) => {
            let content = line_to_string(&deletion.line)?;
            [
                format::negative(deletion.line_no).into(),
                format::default(String::new()).into(),
                format::negative(format!("- {content}")).into(),
            ]
        }
        Modification::Context {
            line,
            line_no_old,
            line_no_new,
        } => {
            let content = line_to_string(line)?;
            [
                format::faint(line_no_old).into(),
                format::faint(line_no_new).into(),
                format::default(format!("  {content}")).into(),
            ]
        }
    };

    Ok(row)
}

fn message(message: &str) -> anyhow::Result<[crate::Line; 3]> {
    let row = [
        format::default(String::new()).into(),
        format::default(String::new()).into(),
        format::default(message).into(),
    ];

    Ok(row)
}

pub fn line_to_string(line: &Line) -> Result<String, anyhow::Error> {
    let unescaped = serde_json::to_string(line)?
        .replace("\\n", "")
        .replace("\\", "");
    let stripped = unescaped
        .strip_prefix('"')
        .unwrap()
        .strip_suffix('"')
        .unwrap();

    Ok(stripped.to_string())
}
