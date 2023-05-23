use radicle::cob::issue::Issue;
use radicle::cob::issue::IssueId;
use radicle::Profile;
use tuirealm::props::Color;

use super::common;
use super::Widget;

use crate::ui::cob;
use crate::ui::components;
use crate::ui::components::common::context::ContextBar;
use crate::ui::components::issue::Preview;
use crate::ui::theme::Theme;

pub fn preview(theme: &Theme, issue: (IssueId, &Issue), profile: &Profile) -> Widget<Preview> {
    let (id, issue) = issue;
    let shortcuts = common::shortcuts(
        theme,
        vec![
            common::shortcut(theme, "esc", "back"),
            common::shortcut(theme, "q", "quit"),
        ],
    );
    let context = context(theme, (id, issue), profile);

    let not_implemented = components::label("not implemented").foreground(theme.colors.default_fg);
    let activity = Preview::new(not_implemented, context, shortcuts);

    Widget::new(activity)
}

pub fn context(theme: &Theme, issue: (IssueId, &Issue), profile: &Profile) -> Widget<ContextBar> {
    let (id, issue) = issue;
    let is_you = *issue.author().id() == profile.did();

    let id = cob::format_id(&id);
    let title = issue.title();
    let author = cob::format_author(issue.author().id(), is_you);
    let comments = issue.comments().count();

    let context = components::label(" issue ").background(theme.colors.context_badge_bg);
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
