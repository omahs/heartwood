use tuirealm::props::Attribute;
use tuirealm::MockComponent;

use crate::ui;
use crate::ui::components::common::list::ColumnWidth;

use ui::components;

use ui::components::common::container::{GlobalListener, Header, LabeledContainer, Tabs};
use ui::components::common::context::{Shortcut, Shortcuts};
use ui::components::common::label::Label;
use ui::components::common::list::{Property, PropertyList, TableModel};
use ui::theme::Theme;

use super::Widget;

pub fn global_listener() -> Widget<GlobalListener> {
    Widget::new(GlobalListener::default())
}

pub fn container_header(theme: &Theme, label: Widget<Label>) -> Widget<Header<(), 1>> {
    let model = TableModel::new([label], [ColumnWidth::Grow]);

    Widget::new(Header::new(model, theme.clone(), 0))
}

pub fn labeled_container(
    theme: &Theme,
    title: &str,
    component: Box<dyn MockComponent>,
) -> Widget<LabeledContainer> {
    let header = container_header(
        theme,
        components::label(&format!(" {title} ")).foreground(theme.colors.default_fg),
    );
    let container = LabeledContainer::new(header, component);

    Widget::new(container)
}

pub fn shortcut(theme: &Theme, short: &str, long: &str) -> Widget<Shortcut> {
    let short = components::label(short).foreground(theme.colors.shortcut_short_fg);
    let divider = components::label(&theme.icons.whitespace.to_string());
    let long = components::label(long).foreground(theme.colors.shortcut_long_fg);

    // TODO: Remove when size constraints are implemented
    let short_w = short.query(Attribute::Width).unwrap().unwrap_size();
    let divider_w = divider.query(Attribute::Width).unwrap().unwrap_size();
    let long_w = long.query(Attribute::Width).unwrap().unwrap_size();
    let width = short_w.saturating_add(divider_w).saturating_add(long_w);

    let shortcut = Shortcut::new(short, divider, long);

    Widget::new(shortcut).height(1).width(width)
}

pub fn shortcuts(theme: &Theme, shortcuts: Vec<Widget<Shortcut>>) -> Widget<Shortcuts> {
    let divider = components::label(&format!(" {} ", theme.icons.shortcutbar_divider))
        .foreground(theme.colors.shortcutbar_divider_fg);
    let shortcut_bar = Shortcuts::new(shortcuts, divider);

    Widget::new(shortcut_bar).height(1)
}

pub fn property(theme: &Theme, name: &str, value: &str) -> Widget<Property> {
    let name = components::label(name).foreground(theme.colors.property_name_fg);
    let divider = components::label(&format!(" {} ", theme.icons.property_divider));
    let value = components::label(value).foreground(theme.colors.default_fg);

    // TODO: Remove when size constraints are implemented
    let name_w = name.query(Attribute::Width).unwrap().unwrap_size();
    let divider_w = divider.query(Attribute::Width).unwrap().unwrap_size();
    let value_w = value.query(Attribute::Width).unwrap().unwrap_size();
    let width = name_w.saturating_add(divider_w).saturating_add(value_w);

    let property = Property::new(name, divider, value);

    Widget::new(property).height(1).width(width)
}

pub fn property_list(_theme: &Theme, properties: Vec<Widget<Property>>) -> Widget<PropertyList> {
    let property_list = PropertyList::new(properties);

    Widget::new(property_list)
}

pub fn tabs(theme: &Theme, tabs: Vec<Widget<Label>>) -> Widget<Tabs> {
    let line = components::label(&theme.icons.tab_overline.to_string())
        .foreground(theme.colors.tabs_highlighted_fg);
    let tabs = Tabs::new(tabs, line);

    Widget::new(tabs).height(2)
}
