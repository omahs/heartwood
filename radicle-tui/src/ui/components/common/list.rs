use tuirealm::command::{Cmd, CmdResult};
use tuirealm::props::{AttrValue, Attribute, BorderSides, BorderType, Color, Props, Style};
use tuirealm::tui::layout::{Constraint, Direction, Layout, Rect};
use tuirealm::tui::widgets::{Block, Cell, Row, TableState};
use tuirealm::{Frame, MockComponent, State, StateValue};

use crate::ui::components::common::label::Label;
use crate::ui::layout;
use crate::ui::theme::Theme;
use crate::ui::widget::{Widget, WidgetComponent};

use super::container::Header;

/// A generic item that can be displayed in a table.
pub trait TableItem {
    /// Get table row applying a given [`theme`].
    fn row<'a>(&self, theme: &Theme) -> Row<'a>;
}

/// Grow behavior of a table column.
///
/// [`tui::widgets::Table`] does only support percental column widths.
/// A [`ColumnWidth`] is used to specify the grow behaviour of a table column
/// and a percental column width is calculated based on that.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ColumnWidth {
    /// A fixed-size column.
    Fixed(u16),
    /// A growable column.
    Grow,
}

/// A generic key-value table model.
///
/// [`K`] needs to implement `ToString` since its string representation
/// is passed to the app's message handler via [`CmdResult`].
/// [`V`] needs to implement `TableItem` in order to be displayed by the
/// table this model is used in.
#[derive(Clone)]
pub struct TableModel<K, V>
where
    K: ToString,
    V: TableItem,
{
    /// The table header.
    header: Vec<Widget<Label>>,
    /// Items hold by this model.
    items: Vec<(K, V)>,
    /// Grow behavior of table columns.
    widths: Vec<ColumnWidth>,
}

impl<K, V> Default for TableModel<K, V>
where
    K: ToString,
    V: TableItem,
{
    fn default() -> Self {
        Self {
            header: vec![],
            items: vec![],
            widths: vec![],
        }
    }
}

impl<K, V> TableModel<K, V>
where
    K: ToString,
    V: TableItem,
{
    /// Adds a new column to this model.
    pub fn with_column(mut self, label: Widget<Label>, width: ColumnWidth) -> Self {
        self.header.push(label);
        self.widths.push(width);
        self
    }

    /// Pushes a new row to this model.
    pub fn push_item(&mut self, item: (K, V)) {
        self.items.push(item);
    }

    /// Get all column widhts defined by this model.
    pub fn widths(&self) -> &Vec<ColumnWidth> {
        &self.widths
    }

    // Get the item count.
    pub fn count(&self) -> u16 {
        self.items.len() as u16
    }

    /// Get this model's table header.
    pub fn header<'a>(&self, theme: &Theme) -> Row<'a> {
        let cells = self.header.iter().map(|label| {
            let cell: Cell = label.into();
            cell.style(Style::default().fg(theme.colors.default_fg))
        });
        Row::new(cells).height(1)
    }

    /// Get this model's table rows.
    pub fn rows<'a>(&self, theme: &Theme) -> Vec<Row<'a>> {
        self.items.iter().map(|(_, item)| item.row(theme)).collect()
    }
}

/// A component that displays a labeled property.
#[derive(Clone)]
pub struct Property {
    label: Widget<Label>,
    divider: Widget<Label>,
    property: Widget<Label>,
}

impl Property {
    pub fn new(label: Widget<Label>, divider: Widget<Label>, property: Widget<Label>) -> Self {
        Self {
            label,
            divider,
            property,
        }
    }
}

impl WidgetComponent for Property {
    fn view(&mut self, properties: &Props, frame: &mut Frame, area: Rect) {
        let display = properties
            .get_or(Attribute::Display, AttrValue::Flag(true))
            .unwrap_flag();

        if display {
            let labels: Vec<Box<dyn MockComponent>> = vec![
                self.label.clone().to_boxed(),
                self.divider.clone().to_boxed(),
                self.property.clone().to_boxed(),
            ];

            let layout = layout::h_stack(labels, area);
            for (mut label, area) in layout {
                label.view(frame, area);
            }
        }
    }

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _properties: &Props, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}

/// A component that can display lists of labeled properties
#[derive(Default)]
pub struct PropertyList {
    properties: Vec<Widget<Property>>,
}

impl PropertyList {
    pub fn new(properties: Vec<Widget<Property>>) -> Self {
        Self { properties }
    }
}

impl WidgetComponent for PropertyList {
    fn view(&mut self, properties: &Props, frame: &mut Frame, area: Rect) {
        let display = properties
            .get_or(Attribute::Display, AttrValue::Flag(true))
            .unwrap_flag();

        if display {
            let properties = self
                .properties
                .iter()
                .map(|property| property.clone().to_boxed() as Box<dyn MockComponent>)
                .collect();

            let layout = layout::v_stack(properties, area);
            for (mut property, area) in layout {
                property.view(frame, area);
            }
        }
    }

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _properties: &Props, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}

/// A table component that can display a list of [`TableItem`]s hold by a [`TableModel`].
pub struct Table<K, V>
where
    K: ToString + Clone,
    V: TableItem + Clone,
{
    model: TableModel<K, V>,
    state: TableState,
    theme: Theme,
    spacing: u16,
}

impl<K, V> Table<K, V>
where
    K: ToString + Clone,
    V: TableItem + Clone,
{
    pub fn new(model: TableModel<K, V>, theme: Theme, spacing: u16) -> Self {
        let mut state = TableState::default();
        state.select(Some(0));
        Self {
            model,
            state,
            theme,
            spacing,
        }
    }

    fn select_previous(&mut self) {
        let index = match self.state.selected() {
            Some(selected) if selected == 0 => 0,
            Some(selected) => selected.saturating_sub(1),
            None => 0,
        };
        self.state.select(Some(index));
    }

    fn select_next(&mut self, len: usize) {
        let index = match self.state.selected() {
            Some(selected) if selected >= len.saturating_sub(1) => len.saturating_sub(1),
            Some(selected) => selected.saturating_add(1),
            None => 0,
        };
        self.state.select(Some(index));
    }

    /// Calculates `Constraint::Percentage` for each fixed column width in `widths`,
    /// taking into account the available width in `area` and the column spacing given by `spacing`.
    pub fn widths(area: Rect, widths: &[ColumnWidth], spacing: u16) -> Vec<Constraint> {
        let total_spacing = spacing.saturating_mul(widths.len() as u16);
        let fixed_width = widths
            .iter()
            .fold(0u16, |total, &width| match width {
                ColumnWidth::Fixed(w) => total + w,
                ColumnWidth::Grow => total,
            })
            .saturating_add(total_spacing);

        let grow_count = widths.iter().fold(0u16, |count, &w| {
            if w == ColumnWidth::Grow {
                count + 1
            } else {
                count
            }
        });
        let grow_width = area
            .width
            .saturating_sub(fixed_width)
            .checked_div(grow_count)
            .unwrap_or(0);

        widths
            .iter()
            .map(|width| match width {
                ColumnWidth::Fixed(w) => {
                    let p: f64 = *w as f64 / area.width as f64 * 100_f64;
                    Constraint::Percentage(p.ceil() as u16)
                }
                ColumnWidth::Grow => {
                    let p: f64 = grow_width as f64 / area.width as f64 * 100_f64;
                    Constraint::Percentage(p.floor() as u16)
                }
            })
            .collect()
    }
}

impl<K, V> WidgetComponent for Table<K, V>
where
    K: ToString + Clone,
    V: TableItem + Clone,
{
    fn view(&mut self, properties: &Props, frame: &mut Frame, area: Rect) {
        let highlight = properties
            .get_or(Attribute::HighlightedColor, AttrValue::Color(Color::Reset))
            .unwrap_color();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        let widths = Self::widths(area, self.model.widths(), self.spacing);
        let table = tuirealm::tui::widgets::Table::new(self.model.rows(&self.theme))
            .block(
                Block::default()
                    .borders(BorderSides::BOTTOM | BorderSides::LEFT | BorderSides::RIGHT)
                    .border_style(Style::default().fg(Color::Rgb(48, 48, 48)))
                    .border_type(BorderType::Rounded),
            )
            .highlight_style(Style::default().bg(highlight))
            .column_spacing(self.spacing)
            .widths(&widths);

        let mut header = Widget::new(Header::new(
            self.model.clone(),
            self.theme.clone(),
            self.spacing,
        ));
        header.view(frame, layout[0]);
        frame.render_stateful_widget(table, layout[1], &mut self.state);
    }

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _properties: &Props, cmd: Cmd) -> CmdResult {
        use tuirealm::command::Direction;

        let len = self.model.count() as usize;
        match cmd {
            Cmd::Move(Direction::Up) => {
                self.select_previous();
                CmdResult::None
            }
            Cmd::Move(Direction::Down) => {
                self.select_next(len);
                CmdResult::None
            }
            Cmd::Submit => {
                let item = self
                    .state
                    .selected()
                    .and_then(|selected| self.model.items.get(selected));
                match item {
                    Some((id, _)) => {
                        CmdResult::Submit(State::One(StateValue::String(id.to_string())))
                    }
                    None => CmdResult::None,
                }
            }
            _ => CmdResult::None,
        }
    }
}
