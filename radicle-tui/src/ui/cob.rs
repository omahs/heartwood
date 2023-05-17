use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use radicle_surf;
use regex::Regex;
use timeago;

use radicle::prelude::{Did, Timestamp};
use radicle::storage::git::Repository;
use radicle::storage::{Oid, ReadRepository};
use radicle::Profile;

use radicle::cob::issue::IssueId;
use radicle::cob::patch::{Patch, PatchId, State};
use radicle::cob::thread::CommentId;

use tuirealm::props::{Color, Style};
use tuirealm::tui::widgets::{Cell, Row};

use crate::ui::components::common::list::TableItem;
use crate::ui::theme::Theme;

/// An author item that can be used in tables, list or trees.
///
/// Breaks up dependencies to [`Profile`] and [`Repository`] that
/// would be needed if [`Author`] would be used directly.
#[derive(Clone)]
pub struct AuthorItem {
    /// The author's DID.
    did: Did,
    /// True if the author is the current user.
    is_you: bool,
}

/// A patch item that can be used in tables, list or trees.
///
/// Breaks up dependencies to [`Profile`] and [`Repository`] that
/// would be needed if [`Patch`] would be used directly.
#[derive(Clone)]
pub struct PatchItem {
    /// Patch OID.
    id: PatchId,
    /// Patch state.
    state: State,
    /// Patch title.
    title: String,
    /// Author of the latest revision.
    author: AuthorItem,
    /// Head of the latest revision.
    head: Oid,
    /// Lines added by the latest revision.
    added: u16,
    /// Lines removed by the latest revision.
    removed: u16,
    /// Time when patch was opened.
    timestamp: Timestamp,
}

impl TryFrom<(&Profile, &Repository, PatchId, Patch)> for PatchItem {
    type Error = anyhow::Error;

    fn try_from(value: (&Profile, &Repository, PatchId, Patch)) -> Result<Self, Self::Error> {
        let (profile, repo, id, patch) = value;
        let (_, rev) = patch.latest().unwrap();
        let repo = radicle_surf::Repository::open(repo.path())?;
        let base = repo.commit(rev.base())?;
        let head = repo.commit(rev.head())?;
        let diff = repo.diff(base.id, head.id)?;

        Ok(PatchItem {
            id,
            state: patch.state(),
            title: patch.title().into(),
            author: AuthorItem {
                did: patch.author().id,
                is_you: *patch.author().id == *profile.did(),
            },
            head: rev.head(),
            added: diff.stats().insertions as u16,
            removed: diff.stats().deletions as u16,
            timestamp: patch.timestamp().as_secs(),
        })
    }
}

impl TableItem for PatchItem {
    fn row<'a>(&self, theme: &Theme) -> Row<'a> {
        let mut cells = vec![];

        let (icon, color) = format_state(&self.state);
        cells.push(Cell::from(icon).style(Style::default().fg(color)));

        cells.push(
            Cell::from(format_id(&self.id))
                .style(Style::default().fg(theme.colors.browser_patch_list_id)),
        );

        cells.push(
            Cell::from(self.title.clone())
                .style(Style::default().fg(theme.colors.browser_patch_list_title)),
        );

        cells.push(
            Cell::from(format_author(&self.author.did, self.author.is_you))
                .style(Style::default().fg(theme.colors.browser_patch_list_author)),
        );

        cells.push(
            Cell::from(format_head(&self.head))
                .style(Style::default().fg(theme.colors.browser_patch_list_head)),
        );

        cells.push(
            Cell::from(format!("{}", self.added))
                .style(Style::default().fg(theme.colors.browser_patch_list_added)),
        );

        cells.push(
            Cell::from(format!("{}", self.removed))
                .style(Style::default().fg(theme.colors.browser_patch_list_removed)),
        );

        cells.push(
            Cell::from(format_timestamp(&self.timestamp))
                .style(Style::default().fg(theme.colors.browser_patch_list_timestamp)),
        );

        Row::new(cells).height(1)
    }
}

impl TableItem for () {
    fn row<'a>(&self, _theme: &Theme) -> Row<'a> {
        let cells: Vec<Cell> = vec![];
        Row::new(cells)
    }
}

/// An id for items that can be displayed in tables, lists and trees,
/// with support for multiple item types.
///
/// Used to map the index of a selected item in a table or list to a
/// string representation that can be passed around by component states.
/// This string representation can be converted back to the actual [`ItemId`]
/// in order to handle each type differently if needed.
#[derive(Clone)]
pub enum ItemId {
    /// An author item, e.g. 'Author(z6MksFq6z5thF2hyhNNSNu1zP2qEL3bHXHZzGH1FLFGAnTgh)'.
    Author(Did),
    /// A comment item, e.g. 'Comment(5e2a83bb1839aae490e88b809a05d8ba312b0483)'.
    Comment(CommentId),
    /// A comment item, e.g. 'Commit(fc951b82c65c58a126e5220c37fc103be540ed53)'.
    Commit(Oid),
    /// An issue item, e.g. 'Issue(f72b448114753222985c1574461f1ee0e7062dfd)'
    Issue(IssueId),
    /// A patch item, e.g. 'Patch(a9c0152a99539653819beec0b80ecae2aac4ce21)'
    Patch(PatchId),
}

impl ToString for ItemId {
    fn to_string(&self) -> String {
        let (name, id) = match self {
            ItemId::Author(did) => ("Author", did.to_human()),
            ItemId::Comment(id) => ("Comment", id.to_string()),
            ItemId::Commit(oid) => ("Commit", oid.to_string()),
            ItemId::Issue(id) => ("Issue", id.to_string()),
            ItemId::Patch(id) => ("Patch", id.to_string()),
        };
        format!("{name}({id})")
    }
}

impl FromStr for ItemId {
    type Err = anyhow::Error;

    fn from_str(from: &str) -> Result<Self, Self::Err> {
        let author = Regex::new(r"Author\((?P<id>[[:alnum:]]{48})\)").unwrap();
        let comment = Regex::new(r"Comment\((?P<id>[[:xdigit:]]{40})\)").unwrap();
        let commit = Regex::new(r"Commit\((?P<id>[[:xdigit:]]{40})\)").unwrap();
        let issue = Regex::new(r"Issue\((?P<id>[[:xdigit:]]{40})\)").unwrap();
        let patch = Regex::new(r"Patch\((?P<id>[[:xdigit:]]{40})\)").unwrap();

        if let Some(cap) = author.captures(from) {
            let did = Did::from_str(&cap["id"])?;
            return Ok(ItemId::Author(did));
        }

        if let Some(cap) = comment.captures(from) {
            let oid = Oid::from_str(&cap["id"])?;
            return Ok(ItemId::Comment(oid.into()));
        }

        if let Some(cap) = commit.captures(from) {
            let oid = Oid::from_str(&cap["id"])?;
            return Ok(ItemId::Comment(oid.into()));
        }

        if let Some(cap) = issue.captures(from) {
            let oid = Oid::from_str(&cap["id"])?;
            return Ok(ItemId::Issue(oid.into()));
        }

        if let Some(cap) = patch.captures(from) {
            let oid = Oid::from_str(&cap["id"])?;
            return Ok(ItemId::Patch(oid.into()));
        }

        Err(anyhow::Error::msg(format!(
            "Could not build ItemId from '{}'",
            from
        )))
    }
}

pub fn format_state(state: &State) -> (String, Color) {
    match state {
        State::Open => (" ● ".into(), Color::Green),
        State::Archived => (" ● ".into(), Color::Yellow),
        State::Draft => (" ● ".into(), Color::Gray),
        State::Merged => (" ✔ ".into(), Color::Blue),
    }
}

pub fn format_id(id: &PatchId) -> String {
    id.to_string()[0..7].to_string()
}

pub fn format_author(did: &Did, is_you: bool) -> String {
    let start = &did.to_human()[0..7];
    let end = &did.to_human()[41..48];

    if is_you {
        format!("{start}…{end} (you)")
    } else {
        format!("{start}…{end}")
    }
}

pub fn format_head(oid: &Oid) -> String {
    oid.to_string()[0..7].to_string()
}

pub fn format_timestamp(timestamp: &Timestamp) -> String {
    let fmt = timeago::Formatter::new();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    fmt.convert(Duration::from_secs(now - timestamp))
}
