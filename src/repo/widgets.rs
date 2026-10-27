//! Small painted widgets shared by the repository panes.

use egui::{Align2, Color32, CornerRadius, Rect, Response, Sense, Ui, pos2, vec2};

use crate::icons;
use crate::theme::{self, Tokens};

pub const ROW_H: f32 = 24.0;

pub fn toolbar_separator(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 20.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.separator);
    ui.add_space(4.0);
}

/// A rounded label with a tinted background.
pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let font = theme::medium(11.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = vec2(galley.size().x + 12.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(9), color.gamma_multiply(0.18));
    ui.painter().galley(
        pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    resp
}

/// The strip at the top of a side panel, like PhotoCraft's panel tab strips: a tab with the
/// title, and room on the right for buttons.
pub fn panel_header(ui: &mut Ui, title: &str, add_buttons: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, t.tab_strip);
    let font = theme::semibold(11.5);
    let galley = painter.layout_no_wrap(title.to_owned(), font, t.text);
    let tab = Rect::from_min_size(
        pos2(rect.left() + 4.0, rect.top() + 4.0),
        vec2(galley.size().x + 24.0, rect.height() - 4.0),
    );
    painter.rect_filled(
        tab,
        CornerRadius {
            nw: t.radius as u8,
            ne: t.radius as u8,
            sw: 0,
            se: 0,
        },
        t.card,
    );
    painter.galley(
        pos2(tab.left() + 12.0, tab.center().y - galley.size().y / 2.0),
        galley,
        t.text,
    );
    let buttons = Rect::from_min_max(
        pos2(tab.right() + 4.0, rect.top()),
        rect.max - vec2(6.0, 0.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(buttons)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    add_buttons(&mut child);
}

/// A collapsible section header (`▸ ICON TITLE  count`). Returns whether it is open.
pub fn section(
    ui: &mut Ui,
    id: &str,
    icon: &str,
    title: &str,
    count: usize,
    default_open: bool,
) -> bool {
    section_with_button(ui, id, icon, title, count, default_open, None).0
}

/// [`section`] with an icon button on the header, left of the count: `(icon, tooltip)`.
/// Returns (open, button clicked).
pub fn section_with_button(
    ui: &mut Ui,
    id: &str,
    icon: &str,
    title: &str,
    count: usize,
    default_open: bool,
    button: Option<(&str, &str)>,
) -> (bool, bool) {
    let t = Tokens::get(ui.ctx());
    let id = ui.id().with(("section", id));
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(default_open);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 0.0, t.hover);
    }
    let y = rect.center().y;
    let chevron = if open {
        "chevron-down"
    } else {
        "chevron-right"
    };
    icons::paint(
        ui,
        Rect::from_center_size(pos2(rect.left() + 14.0, y), vec2(14.0, 14.0)),
        chevron,
        12.0,
        t.text_faint,
    );
    icons::paint(
        ui,
        Rect::from_center_size(pos2(rect.left() + 32.0, y), vec2(14.0, 14.0)),
        icon,
        14.0,
        t.icon,
    );
    painter.text(
        pos2(rect.left() + 46.0, y),
        Align2::LEFT_CENTER,
        title,
        theme::semibold(11.5),
        t.text_dim,
    );
    painter.text(
        pos2(rect.right() - 10.0, y),
        Align2::RIGHT_CENTER,
        count.to_string(),
        theme::medium(11.0),
        t.text_faint,
    );
    let mut clicked = false;
    if let Some((icon, tooltip)) = button {
        let r = Rect::from_center_size(pos2(rect.right() - 36.0, y), vec2(20.0, 20.0));
        let b = ui
            .interact(r, id.with("button"), Sense::click())
            .on_hover_text(tooltip);
        if b.hovered() {
            ui.painter().rect_filled(r, t.radius_sm, t.pressed);
        }
        icons::paint(ui, r, icon, 13.0, if b.hovered() { t.text } else { t.icon });
        clicked = b.clicked();
    }
    (open, clicked)
}

pub fn status_color(t: &Tokens, code: char) -> Color32 {
    match code {
        'A' | '?' => t.success,
        'D' => t.danger,
        'R' | 'C' => t.accent,
        'U' => t.danger,
        _ => t.warning,
    }
}

/// What a file row shows on its right edge.
pub enum RowExtra<'a> {
    /// `+added −removed` (`None` for binary files).
    Stats(Option<u32>, Option<u32>),
    /// Icon buttons shown while the row is hovered: `(icon, tooltip)`.
    Buttons(&'a [(&'a str, &'a str)]),
}

pub struct FileRowResponse {
    pub row: Response,
    /// Index of the clicked hover button.
    pub button: Option<usize>,
}

/// One changed file: status letter, file name, its folder dimmed, and `extra` on the right.
pub fn file_row(
    ui: &mut Ui,
    code: char,
    path: &str,
    old_path: Option<&str>,
    selected: bool,
    extra: RowExtra<'_>,
) -> FileRowResponse {
    let t = Tokens::get(ui.ctx());
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 0.0, t.row_selected);
    } else if hovered {
        painter.rect_filled(rect, 0.0, t.hover);
    }
    let y = rect.center().y;

    // Status letter in a small rounded square.
    let color = status_color(&t, code);
    let badge = Rect::from_center_size(pos2(rect.left() + 18.0, y), vec2(16.0, 16.0));
    painter.rect_filled(badge, CornerRadius::same(3), color.gamma_multiply(0.22));
    let letter = if code == '?' { 'A' } else { code };
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        letter.to_string(),
        theme::semibold(10.5),
        color,
    );

    // Right edge first, so the name is clipped before it.
    let mut right = rect.right() - 8.0;
    let mut button = None;
    match extra {
        RowExtra::Stats(added, removed) => {
            let small = theme::mono(11.0);
            match (added, removed) {
                (Some(a), Some(d)) => {
                    let r = painter.text(
                        pos2(right, y),
                        Align2::RIGHT_CENTER,
                        format!("−{d}"),
                        small.clone(),
                        t.danger,
                    );
                    right = r.left() - 6.0;
                    let r = painter.text(
                        pos2(right, y),
                        Align2::RIGHT_CENTER,
                        format!("+{a}"),
                        small,
                        t.success,
                    );
                    right = r.left() - 8.0;
                }
                _ => {
                    let r = painter.text(
                        pos2(right, y),
                        Align2::RIGHT_CENTER,
                        "bin",
                        small,
                        t.text_faint,
                    );
                    right = r.left() - 8.0;
                }
            }
        }
        RowExtra::Buttons(buttons) => {
            if hovered || selected {
                for (i, (icon, tooltip)) in buttons.iter().enumerate().rev() {
                    let r = Rect::from_center_size(pos2(right - 10.0, y), vec2(20.0, 20.0));
                    let resp = ui
                        .interact(r, row.id.with(("btn", i)), Sense::click())
                        .on_hover_text(*tooltip);
                    if resp.hovered() {
                        painter.rect_filled(r, t.radius_sm, t.pressed);
                    }
                    icons::paint(
                        ui,
                        r,
                        icon,
                        14.0,
                        if resp.hovered() { t.text } else { t.icon },
                    );
                    if resp.clicked() {
                        button = Some(i);
                    }
                    right = r.left() - 2.0;
                }
            }
        }
    }

    let (dir, name) = match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    };
    let text_clip = Rect::from_min_max(
        pos2(rect.left() + 32.0, rect.top()),
        pos2(right, rect.bottom()),
    );
    let tp = painter.with_clip_rect(text_clip);
    let name_rect = tp.text(
        pos2(text_clip.left(), y),
        Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(12.5),
        t.text,
    );
    let mut faint = String::new();
    if let Some(old) = old_path {
        faint.push_str("← ");
        faint.push_str(old);
        faint.push_str("  ");
    }
    faint.push_str(dir);
    if !faint.is_empty() {
        tp.text(
            pos2(name_rect.right() + 8.0, y),
            Align2::LEFT_CENTER,
            faint,
            egui::FontId::proportional(11.5),
            t.text_faint,
        );
    }
    // The full path only adds something when the row shows it cut short.
    let row = match old_path {
        Some(old) => row.on_hover_text(format!("{old} → {path}")),
        None if !dir.is_empty() => row.on_hover_text(path),
        None => row,
    };
    FileRowResponse { row, button }
}

/// A primary (accent-filled) button.
pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let fill = if enabled { t.accent } else { t.hover };
    let text_color = if enabled { t.accent_text } else { t.text_faint };
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(text)
                .color(text_color)
                .font(theme::medium(12.5)),
        )
        .fill(fill)
        .min_size(vec2(width, 30.0)),
    )
}

/// A round initials avatar, coloured by name so a person keeps their colour everywhere.
pub fn avatar(painter: &egui::Painter, center: egui::Pos2, radius: f32, name: &str) {
    let t = Tokens::get(painter.ctx());
    painter.circle_filled(center, radius, t.lane(name_hash(name)));
    painter.text(
        center,
        Align2::CENTER_CENTER,
        initials(name),
        theme::semibold(radius * 0.8),
        Color32::WHITE,
    );
}

fn initials(name: &str) -> String {
    let mut parts = name.split_whitespace().filter_map(|w| w.chars().next());
    let first = parts.next().unwrap_or('?');
    let last = parts.next_back();
    let mut s: String = first.to_uppercase().collect();
    if let Some(l) = last {
        s.extend(l.to_uppercase());
    }
    s
}

fn name_hash(name: &str) -> usize {
    name.bytes()
        .fold(7usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))
}
