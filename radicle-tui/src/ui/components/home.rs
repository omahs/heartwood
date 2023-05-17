use radicle::cob::patch::Patches;
use radicle::Profile;

use radicle::storage::ReadStorage;

use radicle::prelude::Id;
use tuirealm::command::{Cmd, CmdResult};
use tuirealm::props::Props;
use tuirealm::tui::layout::Rect;
use tuirealm::{AttrValue, Attribute, Frame, MockComponent, State};

use crate::ui::cob::PatchItem;
use crate::ui::layout;
use crate::ui::theme::Theme;
use crate::ui::widget::{Widget, WidgetComponent};

use super::common::container::LabeledContainer;
use super::common::context::Shortcuts;
use super::common::label::Label;
use super::common::list::{ColumnWidth, Table, TableModel};
use super::*;

pub struct Dashboard {
    about: Widget<LabeledContainer>,
    shortcuts: Widget<Shortcuts>,
}

impl Dashboard {
    pub fn new(about: Widget<LabeledContainer>, shortcuts: Widget<Shortcuts>) -> Self {
        Self { about, shortcuts }
    }
}

impl WidgetComponent for Dashboard {
    fn view(&mut self, _properties: &Props, frame: &mut Frame, area: Rect) {
        let shortcuts_h = self
            .shortcuts
            .query(Attribute::Height)
            .unwrap_or(AttrValue::Size(0))
            .unwrap_size();
        let layout = layout::root_component(area, shortcuts_h);

        self.about.view(frame, layout[0]);
        self.shortcuts.view(frame, layout[1]);
    }

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _properties: &Props, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}

pub struct IssueBrowser {
    label: Widget<Label>,
    shortcuts: Widget<Shortcuts>,
}

impl IssueBrowser {
    pub fn new(label: Widget<Label>, shortcuts: Widget<Shortcuts>) -> Self {
        Self { label, shortcuts }
    }
}

impl WidgetComponent for IssueBrowser {
    fn view(&mut self, _properties: &Props, frame: &mut Frame, area: Rect) {
        let label_w = self
            .label
            .query(Attribute::Width)
            .unwrap_or(AttrValue::Size(1))
            .unwrap_size();
        let shortcuts_h = self
            .shortcuts
            .query(Attribute::Height)
            .unwrap_or(AttrValue::Size(0))
            .unwrap_size();
        let layout = layout::root_component(area, shortcuts_h);

        self.label
            .view(frame, layout::centered_label(label_w, layout[0]));
        self.shortcuts.view(frame, layout[1])
    }

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _properties: &Props, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}

pub struct PatchBrowser {
    table: Widget<Table<PatchItem, 8>>,
    shortcuts: Widget<Shortcuts>,
}

impl PatchBrowser {
    pub fn new(theme: &Theme, profile: &Profile, id: &Id, shortcuts: Widget<Shortcuts>) -> Self {
        let repo = profile.storage.repository(*id).unwrap();
        let patches = Patches::open(&repo).unwrap();

        let mut model = TableModel::new(
            [
                label(" ● "),
                label("ID"),
                label("Title"),
                label("Author"),
                label("Head"),
                label("+"),
                label("-"),
                label("Updated"),
            ],
            [
                ColumnWidth::Fixed(3),
                ColumnWidth::Fixed(7),
                ColumnWidth::Grow,
                ColumnWidth::Fixed(21),
                ColumnWidth::Fixed(7),
                ColumnWidth::Fixed(4),
                ColumnWidth::Fixed(4),
                ColumnWidth::Fixed(18),
            ],
        );

        if let Ok(all) = patches.all() {
            let mut patches = all.flatten().collect::<Vec<_>>();
            patches.sort_by(|(_, a, _), (_, b, _)| b.timestamp().cmp(&a.timestamp()));
            patches.sort_by(|(_, a, _), (_, b, _)| a.state().cmp(b.state()));

            for (id, patch, _) in patches {
                if let Ok(item) = PatchItem::try_from((profile, &repo, id, patch)) {
                    model.push_item(item);
                }
            }
        }

        let table = Widget::new(Table::new(model, theme.clone(), 2))
            .highlight(theme.colors.item_list_highlighted_bg);
        Self { table, shortcuts }
    }

    pub fn selected_item(&self) -> Option<&PatchItem> {
        self.table.selection()
    }
}

impl WidgetComponent for PatchBrowser {
    fn view(&mut self, _properties: &Props, frame: &mut Frame, area: Rect) {
        let shortcuts_h = self
            .shortcuts
            .query(Attribute::Height)
            .unwrap_or(AttrValue::Size(0))
            .unwrap_size();
        let layout = layout::root_component(area, shortcuts_h);

        self.table.view(frame, layout[0]);
        self.shortcuts.view(frame, layout[1]);
    }

    fn state(&self) -> State {
        self.table.state()
    }

    fn perform(&mut self, _properties: &Props, cmd: Cmd) -> CmdResult {
        self.table.perform(cmd)
    }
}
