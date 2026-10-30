use adw::prelude::*;
use gtk::{
    Align, AspectFrame, Box, Button, CenterBox, Image, Label, Orientation, Picture, ScrolledWindow,
};

// Icon + label inside a button.
//
// Layout: [icon][  label  ][spacer]. The label is the CenterBox's center child, so it sits at the
// middle of the whole button, and the spacer on the right balances the icon on the left. The
// spacer is `LABEL_SHIFT` px narrower than the icon slot, which nudges the label slightly to the
// right (by half of `LABEL_SHIFT`) so it looks centered in the space that remains next to the
// icon. Use 0 for a mathematically centered label; larger values move it further right.
const ICON_SIZE: i32 = 16;
const ICON_GAP: i32 = 8;
const LABEL_SHIFT: i32 = 8;

fn set_icon_label(button: &Button, label: &str, icon_name: &str, icon_halign: Option<Align>) {
    let content = CenterBox::new();
    content.set_hexpand(true);

    let icon = symbolic_image(icon_name);
    icon.set_pixel_size(ICON_SIZE);
    icon.set_valign(Align::Center);
    icon.set_halign(icon_halign.unwrap_or(Align::Start));
    icon.set_margin_end(ICON_GAP);
    content.set_start_widget(Some(&icon));

    let text = Label::new(Some(label));
    text.set_halign(Align::Center);
    text.set_valign(Align::Center);
    text.set_justify(gtk::Justification::Center);
    content.set_center_widget(Some(&text));

    let spacer = Box::new(Orientation::Horizontal, 0);
    spacer.set_size_request((ICON_SIZE + ICON_GAP - LABEL_SHIFT).max(0), 1);
    content.set_end_widget(Some(&spacer));

    button.set_child(Some(&content));
}

pub(crate) fn set_game_action_button(button: &Button, label: &str, icon_name: &str) {
    set_icon_label(button, label, icon_name, Some(Align::Start));
}

pub(crate) fn page_scroller() -> (ScrolledWindow, Box) {
    let scroll = ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    let content = Box::new(Orientation::Vertical, 18);
    content.set_margin_top(28);
    content.set_margin_bottom(36);
    crate::layout::apply_page_padding(&content);
    scroll.set_child(Some(&content));
    (scroll, content)
}

pub(crate) fn styled_picture() -> Picture {
    let picture = Picture::new();
    picture.add_css_class("berry-image");
    picture.set_overflow(gtk::Overflow::Hidden);
    picture
}

/// A picture that fills all the space it is given, cropping to cover it.
pub(crate) fn cover_picture() -> Picture {
    let picture = styled_picture();
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Fill);
    picture.set_valign(Align::Fill);
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture
}

pub(crate) fn resource_picture(path: &str) -> Picture {
    let picture = styled_picture();
    picture.set_paintable(berry_media::images::catalog_paintable(path).as_ref());
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture
}

pub(crate) fn symbolic_image(icon_name: &str) -> Image {
    let image = Image::from_icon_name(icon_name);
    image.set_pixel_size(16);
    image
}

pub(crate) fn rounded_card_picture(path: &str, width: i32, height: i32) -> AspectFrame {
    let frame = AspectFrame::new(0.5, 0.5, width as f32 / height as f32, false);
    frame.add_css_class("berry-card-media");
    frame.set_size_request(width, height);
    frame.set_overflow(gtk::Overflow::Hidden);

    let picture = resource_picture(path);
    picture.set_content_fit(gtk::ContentFit::Cover);
    frame.set_child(Some(&picture));
    frame
}

pub(crate) fn set_library_action_button(
    button: &Button,
    label: &str,
    icon_name: &str,
    destructive: bool,
) {
    set_icon_label(button, label, icon_name, None);
    button.remove_css_class("suggested-action");
    button.remove_css_class("destructive-action");
    button.add_css_class(if destructive {
        "destructive-action"
    } else {
        "suggested-action"
    });
}

pub(crate) fn library_action_button(label: &str, icon_name: &str, destructive: bool) -> Button {
    let button = Button::new();
    set_library_action_button(&button, label, icon_name, destructive);
    button
}

/// A poster picture in a frame of `ratio` (width / height) that reports its height to the parent.
pub(crate) fn resource_picture_in_aspect_frame(path: &str, ratio: f64) -> Box {
    crate::ratio_layout::ratio_frame(&resource_picture(path), ratio)
}

pub(crate) fn selected_string(dropdown: &gtk::DropDown) -> Option<String> {
    dropdown
        .selected_item()
        .and_then(|item| item.downcast::<gtk::StringObject>().ok())
        .map(|item| item.string().to_string())
}

/// A preferences row with one control at its end, vertically centered and not stretched.
pub(crate) fn suffix_row(
    title: &str,
    subtitle: Option<&str>,
    suffix: &impl IsA<gtk::Widget>,
) -> adw::ActionRow {
    suffix.set_valign(Align::Center);
    suffix.set_hexpand(false);
    let row = adw::ActionRow::new();
    row.set_title(title);
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }
    row.add_suffix(suffix);
    row
}
