use tuirealm::command::{Cmd, CmdResult};
use tuirealm::props::{AttrValue, Attribute, BorderSides, BorderType, Color, Props, Style};
use tuirealm::tui::layout::{Constraint, Direction, Layout, Rect};
use tuirealm::tui::widgets::{Block, Cell, ListState, Row, TableState};
use tuirealm::{Frame, MockComponent, State};

use crate::ui::components::common::label::Label;
use crate::ui::layout;
use crate::ui::theme::Theme;
use crate::ui::widget::{Widget, WidgetComponent};

use super::container::Header;

/// A generic item that can be displayed in a table with [`W`] columns.
pub trait TableItem<const W: usize> {
    /// Should return fields as table cells.
    fn row(&self, theme: &Theme) -> [Cell; W];
}

/// Grow behavior of a table column.
///
/// [`tui::widgets::Table`] does only support percental column widths.
/// A [`ColumnWidth`] is used to specify the grow behaviour of a table column
/// and a percental column width is calculated based on that.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColumnWidth {
    /// A fixed-size column.
    Fixed(u16),
    /// A growable column.
    Grow,
}

/// A generic table model with [`W`] columns.
///
/// [`V`] needs to implement `TableItem` in order to be displayed by the
/// table this model is used in.
#[derive(Clone)]
pub struct TableModel<V, const W: usize>
where
    V: TableItem<W>,
{
    /// The table header.
    header: [Widget<Label>; W],
    /// Grow behavior of table columns.
    widths: [ColumnWidth; W],
    /// Items hold by this model.
    items: Vec<V>,
}

impl<V, const W: usize> TableModel<V, W>
where
    V: TableItem<W>,
{
    pub fn new(header: [Widget<Label>; W], widths: [ColumnWidth; W]) -> Self {
        Self {
            header,
            widths,
            items: vec![],
        }
    }

    /// Pushes a new row to this model.
    pub fn push_item(&mut self, item: V) {
        self.items.push(item);
    }

    /// Get all column widhts defined by this model.
    pub fn widths(&self) -> &[ColumnWidth; W] {
        &self.widths
    }

    // Get the item count.
    pub fn count(&self) -> u16 {
        self.items.len() as u16
    }

    /// Get this model's table header.
    pub fn header(&self, theme: &Theme) -> [Cell; W] {
        self.header
            .iter()
            .map(|label| {
                let cell: Cell = label.into();
                cell.style(Style::default().fg(theme.colors.default_fg))
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }

    /// Get this model's table rows.
    pub fn rows(&self, theme: &Theme) -> Vec<[Cell; W]> {
        self.items.iter().map(|item| item.row(theme)).collect()
    }
}

/// A generic item that can be displayed in a list.
pub trait ListItem {
    /// Should return fields as list item.
    fn row(&self, theme: &Theme) -> tuirealm::tui::widgets::ListItem;
}

/// A generic list model.
///
/// [`V`] needs to implement `ListItem` in order to be displayed by the
/// list this model is used in.
#[derive(Clone)]
pub struct ListModel<V>
where
    V: ListItem,
{
    /// The table header.
    header: Widget<Label>,
    /// Items hold by this model.
    items: Vec<V>,
}

impl<V> ListModel<V>
where
    V: ListItem,
{
    pub fn new(header: Widget<Label>) -> Self {
        Self {
            header,
            items: vec![],
        }
    }

    /// Pushes a new row to this model.
    pub fn push_item(&mut self, item: V) {
        self.items.push(item);
    }

    // Get the item count.
    pub fn count(&self) -> u16 {
        self.items.len() as u16
    }

    /// Get this model's table header.
    pub fn header(&self, _theme: &Theme) -> Widget<Label> {
        self.header.clone()
    }

    /// Get this model's table rows.
    pub fn rows(&self, theme: &Theme) -> Vec<tuirealm::tui::widgets::ListItem> {
        self.items.iter().map(|item| item.row(theme)).collect()
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
pub struct Table<V, const W: usize>
where
    V: TableItem<W> + Clone,
{
    model: TableModel<V, W>,
    state: TableState,
    theme: Theme,
    spacing: u16,
}

impl<V, const W: usize> Table<V, W>
where
    V: TableItem<W> + Clone,
{
    pub fn new(model: TableModel<V, W>, theme: Theme, spacing: u16) -> Self {
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

    pub fn selection(&self) -> Option<&V> {
        self.state
            .selected()
            .and_then(|selected| self.model.items.get(selected))
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

impl<V, const W: usize> WidgetComponent for Table<V, W>
where
    V: TableItem<W> + Clone,
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
        let rows: Vec<Row<'_>> = self
            .model
            .rows(&self.theme)
            .iter()
            .map(|cells| Row::new(cells.clone()))
            .collect();

        let mut header = Widget::new(Header::new(
            self.model.clone(),
            self.theme.clone(),
            self.spacing,
        ));
        let table = tuirealm::tui::widgets::Table::new(rows)
            .block(
                Block::default()
                    .borders(BorderSides::BOTTOM | BorderSides::LEFT | BorderSides::RIGHT)
                    .border_style(Style::default().fg(Color::Rgb(48, 48, 48)))
                    .border_type(BorderType::Rounded),
            )
            .highlight_style(Style::default().bg(highlight))
            .column_spacing(self.spacing)
            .widths(&widths);

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
            _ => CmdResult::None,
        }
    }
}

////////////////////////////////////////////////

/// A table component that can display a list of [`TableItem`]s hold by a [`TableModel`].
pub struct List<V>
where
    V: ListItem + Clone,
{
    model: ListModel<V>,
    state: ListState,
    theme: Theme,
    spacing: u16,
}

impl<V> List<V>
where
    V: ListItem + Clone,
{
    pub fn new(model: ListModel<V>, theme: Theme, selection: usize, spacing: u16) -> Self {
        let mut state = ListState::default();
        state.select(Some(selection));
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

    pub fn selection(&self) -> Option<&V> {
        self.state
            .selected()
            .and_then(|selected| self.model.items.get(selected))
    }
}

impl<V> WidgetComponent for List<V>
where
    V: ListItem + Clone,
{
    fn view(&mut self, properties: &Props, frame: &mut Frame, area: Rect) {
        let highlight = properties
            .get_or(Attribute::HighlightedColor, AttrValue::Color(Color::Reset))
            .unwrap_color();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        let mut header = Widget::new(Header::new(
            TableModel::<(), 1>::new([self.model.header.clone()], [ColumnWidth::Grow]),
            self.theme.clone(),
            self.spacing,
        ));
        header.view(frame, layout[0]);

        let rows = self.model.rows(&self.theme);
        let list =
            tuirealm::tui::widgets::List::new(rows).highlight_style(Style::default().bg(highlight));

        let block = Block::default()
            .borders(BorderSides::BOTTOM | BorderSides::LEFT | BorderSides::RIGHT)
            .border_style(Style::default().fg(Color::Rgb(48, 48, 48)))
            .border_type(BorderType::Rounded);
        frame.render_widget(block, layout[1]);

        let mut inner_area = layout[1];
        inner_area.height -= 1;

        let inner = Layout::default()
            .horizontal_margin(self.spacing)
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage(100)])
            .split(inner_area);
        frame.render_stateful_widget(list, inner[0], &mut self.state);
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
            _ => CmdResult::None,
        }
    }
}
