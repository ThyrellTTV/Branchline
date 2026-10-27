//! Lucide icons (ISC, see `assets/icons/LICENSE-lucide.txt`), embedded as SVG, recoloured to
//! white, rasterised by egui_extras at the on-screen size and tinted per use. Same approach as
//! PhotoCraft: no Unicode glyphs, which half the time render as boxes.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use egui::{Color32, Rect, Response, RichText, Sense, Ui, Vec2, load::Bytes};

use crate::theme::Tokens;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../assets/icons/", $name, ".svg")))),*]
    };
}

const SOURCES: &[(&str, &str)] = icons![
    "archive",
    "arrow-down",
    "arrow-down-to-line",
    "arrow-up",
    "arrow-up-from-line",
    "check",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "circle-alert",
    "cloud",
    "cloud-download",
    "columns-2",
    "command",
    "copy",
    "external-link",
    "file-diff",
    "folder-git-2",
    "folder-open",
    "folder-plus",
    "folder-tree",
    "git-branch",
    "git-branch-plus",
    "git-merge",
    "git-pull-request",
    "laptop",
    "log-in",
    "minus",
    "palette",
    "pilcrow",
    "plus",
    "redo-2",
    "refresh-cw",
    "rows-2",
    "search",
    "settings",
    "square-terminal",
    "tag",
    "undo-2",
    "user-round",
    "x",
];

fn white_icons() -> &'static HashMap<&'static str, Arc<[u8]>> {
    static MAP: OnceLock<HashMap<&'static str, Arc<[u8]>>> = OnceLock::new();
    MAP.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(name, svg)| {
                let white = svg.replace("currentColor", "#ffffff");
                (*name, Arc::from(white.into_bytes().into_boxed_slice()))
            })
            .collect()
    })
}

/// An egui image for an icon, tinted. Unknown names give an empty image.
pub fn image(name: &str, size: f32, tint: Color32) -> egui::Image<'static> {
    let bytes = white_icons()
        .get(name)
        .cloned()
        .unwrap_or_else(|| Arc::from(Vec::new().into_boxed_slice()));
    egui::Image::from_bytes(format!("bytes://icons/{name}.svg"), Bytes::Shared(bytes))
        .fit_to_exact_size(Vec2::splat(size))
        .tint(tint)
}

/// Paints an icon centred in `rect`.
pub fn paint(ui: &Ui, rect: Rect, name: &str, size: f32, tint: Color32) {
    let r = Rect::from_center_size(rect.center(), Vec2::splat(size));
    image(name, size, tint).paint_at(ui, r);
}

/// A flat toolbar button: icon plus optional label, background only on hover (PhotoCraft's
/// options-bar buttons).
pub fn tool_button(ui: &mut Ui, icon: &str, label: &str, enabled: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let tint = if enabled { t.icon } else { t.text_faint };
    let mut button = egui::Button::image_and_text(
        image(icon, 16.0, tint),
        RichText::new(label).color(if enabled { t.text } else { t.text_faint }),
    )
    .frame_when_inactive(false)
    .min_size(egui::vec2(0.0, 28.0));
    if label.is_empty() {
        button = egui::Button::image(image(icon, 16.0, tint))
            .frame_when_inactive(false)
            .min_size(egui::vec2(28.0, 28.0));
    }
    ui.add_enabled(enabled, button)
}

/// A small square icon-only button (rows, panel headers).
pub fn small_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius_sm, t.pressed);
    }
    let tint = if resp.hovered() { t.text } else { t.icon };
    paint(ui, rect, icon, 14.0, tint);
    resp.on_hover_text(tooltip)
}

/// An icon laid out inline, like a label.
pub fn inline(ui: &mut Ui, name: &str, size: f32, tint: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint(ui, rect, name, size, tint);
}
