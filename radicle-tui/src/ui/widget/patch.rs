use radicle::Profile;
use tuirealm::props::Color;

use radicle::cob::patch::{Patch, PatchId};

use super::common;
use super::Widget;

use crate::ui::cob;
use crate::ui::components;
use crate::ui::components::common::container::Tabs;
use crate::ui::components::common::context::ContextBar;
use crate::ui::components::patch::Activity;
use crate::ui::theme::Theme;

pub fn navigation(theme: &Theme) -> Widget<Tabs> {
    common::tabs(
        theme,
        vec![
            components::reversable_label("activity").foreground(theme.colors.tabs_highlighted_fg),
            components::reversable_label("files").foreground(theme.colors.tabs_highlighted_fg),
        ],
    )
}

pub fn activity(theme: &Theme, patch: (PatchId, &Patch), profile: &Profile) -> Widget<Activity> {
    let (id, patch) = patch;
    let shortcuts = common::shortcuts(
        theme,
        vec![
            common::shortcut(theme, "esc", "back"),
            common::shortcut(theme, "tab", "section"),
            common::shortcut(theme, "q", "quit"),
        ],
    );
    let context = context(theme, (id, patch), profile);

    let not_implemented = components::label("not implemented").foreground(theme.colors.default_fg);
    let activity = Activity::new(not_implemented, context, shortcuts);

    Widget::new(activity)
}

pub fn files(theme: &Theme, patch: (PatchId, &Patch), profile: &Profile) -> Widget<Activity> {
    let (id, patch) = patch;
    let shortcuts = common::shortcuts(
        theme,
        vec![
            common::shortcut(theme, "esc", "back"),
            common::shortcut(theme, "tab", "section"),
            common::shortcut(theme, "q", "quit"),
        ],
    );
    let context = context(theme, (id, patch), profile);

    let not_implemented = components::label("not implemented").foreground(theme.colors.default_fg);
    let files = Activity::new(not_implemented, context, shortcuts);

    Widget::new(files)
}

pub fn context(theme: &Theme, patch: (PatchId, &Patch), profile: &Profile) -> Widget<ContextBar> {
    let (id, patch) = patch;
    let (_, rev) = patch.latest().unwrap();
    let is_you = *patch.author().id() == profile.did();

    let id = cob::format_id(&id);
    let title = patch.title();
    let author = cob::format_author(patch.author().id(), is_you);
    let comments = rev.discussion().len();

    let context = components::label(" patch ").background(theme.colors.context_badge_bg);
    let id = components::label(&format!(" {id} "))
        .foreground(theme.colors.context_id_fg)
        .background(theme.colors.context_id_bg);
    let title = components::label(&format!(" {title} "))
        .foreground(theme.colors.default_fg)
        .background(theme.colors.context_bg);
    let author = components::label(&format!(" {author} "))
        .foreground(theme.colors.context_id_author_fg)
        .background(theme.colors.context_bg);
    let comments = components::label(&format!(" {comments} "))
        .foreground(Color::Rgb(70, 70, 70))
        .background(theme.colors.context_light_bg);

    let context_bar = ContextBar::new(context, id, author, title, comments);

    Widget::new(context_bar).height(1)
}
