use radicle::prelude::Id;
use radicle::Profile;

use radicle::storage::ReadStorage;

use radicle::cob::issue::{IssueId, Issues};

use tuirealm::command::{Cmd, CmdResult};
use tuirealm::props::Props;
use tuirealm::tui::layout::Rect;
use tuirealm::{Frame, MockComponent, State};

use crate::ui::cob::IssueItem;
use crate::ui::theme::Theme;
use crate::ui::widget::{Widget, WidgetComponent};

use super::common::list::{List, ListModel};

use super::*;

pub struct LargeList {
    list: Widget<List<IssueItem>>,
}

impl LargeList {
    pub fn new(theme: &Theme, profile: &Profile, id: &Id, selected: IssueId) -> Self {
        let repo = profile.storage.repository(*id).unwrap();
        let issues = Issues::open(&repo).unwrap();
        let mut model = ListModel::new(label(" Issues "));
        let mut selection = 0;

        if let Ok(all) = issues.all() {
            let mut issues = all.flatten().collect::<Vec<_>>();
            issues.sort_by(|(_, a, _), (_, b, _)| b.timestamp().cmp(&a.timestamp()));
            issues.sort_by(|(_, a, _), (_, b, _)| a.state().cmp(b.state()));

            for (id, issue, _) in issues {
                if let Ok(item) = IssueItem::try_from((profile, &repo, id, issue)) {
                    model.push_item(item);
                    if id == selected {
                        selection = model.count() - 1;
                    }
                }
            }
        }

        let list = Widget::new(List::new(model, theme.clone(), selection as usize, 2))
            .highlight(theme.colors.item_list_highlighted_bg);
        Self { list }
    }
}

impl WidgetComponent for LargeList {
    fn view(&mut self, _properties: &Props, frame: &mut Frame, area: Rect) {
        self.list.view(frame, area);
    }

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _properties: &Props, cmd: Cmd) -> CmdResult {
        self.list.perform(cmd)
    }
}
