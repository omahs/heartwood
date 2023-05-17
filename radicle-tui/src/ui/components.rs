pub mod common;
pub mod home;
pub mod patch;

use tuirealm::AttrValue;

use super::widget::Widget;
use common::label::Label;

pub fn label(content: &str) -> Widget<Label> {
    // TODO: Remove when size constraints are implemented
    let width = content.chars().count() as u16;

    Widget::new(Label::default())
        .content(AttrValue::String(content.to_string()))
        .height(1)
        .width(width)
}

pub fn reversable_label(content: &str) -> Widget<Label> {
    let content = &format!(" {content} ");
    label(content)
}
