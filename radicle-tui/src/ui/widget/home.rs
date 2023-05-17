use radicle::identity::{Id, Project};
use radicle::Profile;

use crate::ui::components;
use crate::ui::components::common::container::Tabs;
use crate::ui::components::home::{Dashboard, IssueBrowser, PatchBrowser};
use crate::ui::theme::Theme;

use super::{common, Widget};

pub fn navigation(theme: &Theme) -> Widget<Tabs> {
    common::tabs(
        theme,
        vec![
            components::reversable_label("dashboard").foreground(theme.colors.tabs_highlighted_fg),
            components::reversable_label("issues").foreground(theme.colors.tabs_highlighted_fg),
            components::reversable_label("patches").foreground(theme.colors.tabs_highlighted_fg),
        ],
    )
}

pub fn dashboard(theme: &Theme, id: &Id, project: &Project) -> Widget<Dashboard> {
    let about = common::labeled_container(
        theme,
        "about",
        common::property_list(
            theme,
            vec![
                common::property(theme, "id", &id.to_string()),
                common::property(theme, "name", project.name()),
                common::property(theme, "description", project.description()),
            ],
        )
        .to_boxed(),
    );
    let shortcuts = common::shortcuts(
        theme,
        vec![
            common::shortcut(theme, "tab", "section"),
            common::shortcut(theme, "q", "quit"),
        ],
    );
    let dashboard = Dashboard::new(about, shortcuts);

    Widget::new(dashboard)
}

pub fn patches(theme: &Theme, id: &Id, profile: &Profile) -> Widget<PatchBrowser> {
    let shortcuts = common::shortcuts(
        theme,
        vec![
            common::shortcut(theme, "tab", "section"),
            common::shortcut(theme, "↑/↓", "navigate"),
            common::shortcut(theme, "enter", "show"),
            common::shortcut(theme, "q", "quit"),
        ],
    );

    Widget::new(PatchBrowser::new(theme, profile, id, shortcuts))
}

pub fn issues(theme: &Theme) -> Widget<IssueBrowser> {
    let shortcuts = common::shortcuts(
        theme,
        vec![
            common::shortcut(theme, "tab", "section"),
            common::shortcut(theme, "q", "quit"),
        ],
    );

    let not_implemented = components::label("not implemented").foreground(theme.colors.default_fg);
    let browser = IssueBrowser::new(not_implemented, shortcuts);

    Widget::new(browser)
}
