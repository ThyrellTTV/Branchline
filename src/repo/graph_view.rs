//! Central pane: the commit graph of every branch.

use egui::{
    Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, Response, Sense, Stroke, Ui,
    epaint::CubicBezierShape, pos2, vec2,
};

use super::{Action, Dialog, Refresh, RepoTab, WIP_HASH, ops};
use crate::git::Refs;
use crate::icons;
use crate::theme::{self, Tokens};

const ROW_H: f32 = 28.0;
const LANE_W: f32 = 16.0;
const MAX_LANES_SHOWN: usize = 24;
const AUTHOR_W: f32 = 150.0;
const DATE_W: f32 = 118.0;

impl RepoTab {
    pub(super) fn graph_view(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        if let Some(err) = &self.load_error {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.colored_label(t.danger, err);
            });
            return;
        }
        if self.commits.is_empty() {
            ui.centered_and_justified(|ui| {
                if self.loading {
                    ui.spinner();
                } else {
                    ui.label(
                        egui::RichText::new("This repository has no commits yet.")
                            .color(t.text_dim),
                    );
                }
            });
            return;
        }

        // Keyboard navigation (only when no text field has focus).
        if !ui.ctx().egui_wants_keyboard_input() && self.dialog.is_none() {
            let (up, down) =
                ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
            let current = self.selected.unwrap_or(0);
            let next = if up {
                Some(current.saturating_sub(1))
            } else if down {
                Some((current + 1).min(self.commits.len().saturating_sub(1)))
            } else {
                None
            };
            if let Some(next) = next {
                acts.push(Action::Select(next));
                self.scroll_to = Some(next);
            }
        }

        let lanes = self.graph.width.clamp(1, MAX_LANES_SHOWN);
        let graph_w = (lanes as f32 * LANE_W + 12.0).max(60.0);

        // Column headers.
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        {
            let p = ui.painter();
            p.rect_filled(header, 0.0, t.chrome);
            p.line_segment(
                [header.left_bottom(), header.right_bottom()],
                Stroke::new(1.0, t.separator),
            );
            let y = header.center().y;
            let font = theme::semibold(10.5);
            for (x, label) in [
                (header.left() + 10.0, "GRAPH"),
                (header.left() + graph_w + 6.0, "DESCRIPTION"),
                (header.right() - DATE_W - AUTHOR_W, "AUTHOR"),
                (header.right() - DATE_W, "DATE"),
            ] {
                p.text(
                    pos2(x, y),
                    Align2::LEFT_CENTER,
                    label,
                    font.clone(),
                    t.text_faint,
                );
            }
        }

        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("graph_scroll")
            .auto_shrink(false);
        let offset_id = ui.id().with("graph_offset");
        if let Some(row) = self.scroll_to.take() {
            let view_h = ui.available_height();
            let top = row as f32 * ROW_H;
            // Only scroll if the row is outside the visible part.
            let offset = ui.data(|d| d.get_temp::<f32>(offset_id)).unwrap_or(0.0);
            if top < offset || top + ROW_H > offset + view_h {
                scroll = scroll.vertical_scroll_offset((top - view_h / 2.0).max(0.0));
            }
        }

        // `show_rows` sizes rows as ROW_H + item spacing, so the spacing must be zero here.
        ui.spacing_mut().item_spacing.y = 0.0;
        let idle = self.busy_op.is_none();
        let out = scroll.show_rows(ui, ROW_H, self.commits.len(), |ui, range| {
            for i in range {
                let resp = self.commit_row(ui, i, graph_w, lanes);
                if resp.clicked() {
                    acts.push(Action::Select(i));
                }
                let Some(commit) = self.commits.get(i) else {
                    continue;
                };
                if commit.hash == WIP_HASH {
                    resp.context_menu(|ui| {
                        if ui
                            .add_enabled(idle, egui::Button::new("Stage all changes"))
                            .clicked()
                        {
                            acts.push(Action::Ops {
                                label: "Stage all".into(),
                                ops: ops(&[&["add", "-A"]]),
                                refresh: Refresh::Status,
                            });
                        }
                    });
                    continue;
                }
                let hash = commit.hash.clone();
                let short = hash.get(..8).unwrap_or(&hash).to_owned();
                let summary = commit.summary.clone();
                let is_head = hash == self.refs.head;
                resp.context_menu(|ui| {
                    if ui.button("Create branch here…").clicked() {
                        acts.push(Action::Dialog(Dialog::create_branch(&hash, &short)));
                    }
                    if ui
                        .add_enabled(
                            idle && !is_head,
                            egui::Button::new("Check out this commit (detached)"),
                        )
                        .clicked()
                    {
                        acts.push(Action::Checkout(hash.clone()));
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            idle && !is_head,
                            egui::Button::new("Cherry-pick onto current branch"),
                        )
                        .clicked()
                    {
                        acts.push(Action::Ops {
                            label: format!("Cherry-pick {short}"),
                            ops: ops(&[&["cherry-pick", &hash]]),
                            refresh: Refresh::All,
                        });
                    }
                    if ui
                        .add_enabled(idle, egui::Button::new("Revert commit"))
                        .clicked()
                    {
                        acts.push(Action::Ops {
                            label: format!("Revert {short}"),
                            ops: ops(&[&["revert", "--no-edit", &hash]]),
                            refresh: Refresh::All,
                        });
                    }
                    ui.separator();
                    if ui.button("Copy commit hash").clicked() {
                        ui.ctx().copy_text(hash.clone());
                    }
                    if ui.button("Copy message").clicked() {
                        ui.ctx().copy_text(summary.clone());
                    }
                });
            }
        });
        ui.data_mut(|d| d.insert_temp(offset_id, out.state.offset.y));
    }

    fn commit_row(&self, ui: &mut Ui, i: usize, graph_w: f32, lanes: usize) -> Response {
        let t = Tokens::get(ui.ctx());
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        let Some(commit) = self.commits.get(i) else {
            return resp;
        };
        let painter = ui.painter_at(rect);
        let is_wip = commit.hash == WIP_HASH;

        if self.selected == Some(i) {
            painter.rect_filled(rect, 0.0, t.row_selected);
        } else if resp.hovered() {
            painter.rect_filled(rect, 0.0, t.hover);
        }

        // Graph.
        let row_color = self
            .graph
            .rows
            .get(i)
            .map(|r| t.lane(r.col))
            .unwrap_or(t.text_faint);
        if let Some(row) = self.graph.rows.get(i) {
            let gp = painter.with_clip_rect(
                rect.intersect(Rect::from_min_size(rect.min, vec2(graph_w, ROW_H))),
            );
            let x = |lane: usize| rect.left() + 6.0 + lane as f32 * LANE_W + LANE_W / 2.0;
            let (top, mid, bottom) = (rect.top(), rect.center().y, rect.bottom());
            let node = pos2(x(row.col), mid);
            let width = 2.0;

            for &lane in &row.through {
                if lane < lanes {
                    gp.line_segment(
                        [pos2(x(lane), top), pos2(x(lane), bottom)],
                        Stroke::new(width, t.lane(lane)),
                    );
                }
            }
            for &lane in &row.incoming {
                curve(&gp, pos2(x(lane), top), node, t.lane(lane), width);
            }
            for &lane in &row.outgoing {
                let color = if is_wip { t.text_faint } else { t.lane(lane) };
                curve(&gp, node, pos2(x(lane), bottom), color, width);
            }

            if is_wip {
                gp.circle_filled(node, 6.0, t.dock);
                gp.add(egui::Shape::dashed_line(
                    &circle_points(node, 6.0, 16),
                    Stroke::new(1.5, t.text_dim),
                    2.5,
                    2.0,
                ));
            } else if commit.parents.len() > 1 {
                gp.circle_filled(node, 3.5, row_color);
            } else {
                gp.circle_filled(node, 6.0, row_color);
                if commit.hash == self.refs.head {
                    gp.circle_filled(node, 2.5, t.dock);
                }
            }
        }

        let mid = rect.center().y;
        let mut x = rect.left() + graph_w + 6.0;
        let text_right = rect.right() - DATE_W - AUTHOR_W - 10.0;

        if is_wip {
            let font = theme::medium(12.5);
            let r = painter.text(
                pos2(x, mid),
                Align2::LEFT_CENTER,
                "Uncommitted changes",
                font,
                t.text,
            );
            x = r.right() + 10.0;
            let conflicts = if self.work.conflicted.len() == 1 {
                "conflict"
            } else {
                "conflicts"
            };
            for (n, label, color) in [
                (self.work.conflicted.len(), conflicts, t.danger),
                (self.work.staged.len(), "staged", t.success),
                (self.work.unstaged.len(), "unstaged", t.warning),
            ] {
                if n > 0 {
                    x += pill(&painter, pos2(x, mid), &format!("{n} {label}"), color) + 6.0;
                }
            }
            return resp;
        }

        // Ref badges, then the summary.
        for badge in badges(&commit.refs, &self.refs) {
            if x > text_right - 40.0 {
                break;
            }
            x += ref_badge(ui, &painter, pos2(x, mid), &badge, row_color, &t) + 4.0;
        }
        painter
            .with_clip_rect(Rect::from_min_max(
                pos2(x, rect.top()),
                pos2(text_right, rect.bottom()),
            ))
            .text(
                pos2(x + 2.0, mid),
                Align2::LEFT_CENTER,
                &commit.summary,
                FontId::proportional(12.5),
                t.text,
            );

        let small = FontId::proportional(11.5);
        let author_clip = Rect::from_min_max(
            pos2(rect.right() - DATE_W - AUTHOR_W, rect.top()),
            pos2(rect.right() - DATE_W - 8.0, rect.bottom()),
        );
        painter.with_clip_rect(author_clip).text(
            pos2(author_clip.left(), mid),
            Align2::LEFT_CENTER,
            &commit.author,
            small.clone(),
            t.text_dim,
        );
        painter.text(
            pos2(rect.right() - DATE_W, mid),
            Align2::LEFT_CENTER,
            &commit.date,
            theme::mono(11.0),
            t.text_faint,
        );
        resp
    }
}

/// One ref badge in the graph.
struct Badge {
    label: String,
    kind: BadgeKind,
    /// The local branch is checked out.
    head: bool,
    /// A local branch whose remote counterpart points at the same commit.
    synced: bool,
}

#[derive(PartialEq)]
enum BadgeKind {
    Local,
    Remote,
    Tag,
    DetachedHead,
}

/// Turns `git log --format=%D` decorations into badges, folding `origin/x` into `x` when both
/// point at the same commit.
fn badges(decorations: &[String], refs: &Refs) -> Vec<Badge> {
    let is_remote = |d: &str| {
        refs.remotes.iter().any(|(remote, _)| {
            d.strip_prefix(remote.as_str())
                .is_some_and(|r| r.starts_with('/'))
        })
    };
    let mut out = Vec::new();
    let locals: Vec<&str> = decorations
        .iter()
        .filter_map(|d| {
            let d = d.strip_prefix("HEAD -> ").unwrap_or(d);
            (!d.starts_with("tag: ") && d != "HEAD" && !is_remote(d)).then_some(d)
        })
        .collect();
    for d in decorations {
        // `origin/HEAD` only says which branch the remote defaults to.
        if is_remote(d) && d.ends_with("/HEAD") {
            continue;
        }
        if let Some(branch) = d.strip_prefix("HEAD -> ") {
            out.push(Badge {
                label: branch.to_owned(),
                kind: BadgeKind::Local,
                head: true,
                synced: false,
            });
        } else if d == "HEAD" {
            out.push(Badge {
                label: "HEAD".to_owned(),
                kind: BadgeKind::DetachedHead,
                head: true,
                synced: false,
            });
        } else if let Some(tag) = d.strip_prefix("tag: ") {
            out.push(Badge {
                label: tag.to_owned(),
                kind: BadgeKind::Tag,
                head: false,
                synced: false,
            });
        } else if is_remote(d) {
            let short = d.split_once('/').map(|(_, b)| b).unwrap_or(d);
            if !locals.contains(&short) {
                out.push(Badge {
                    label: d.clone(),
                    kind: BadgeKind::Remote,
                    head: false,
                    synced: false,
                });
            }
        } else {
            out.push(Badge {
                label: d.clone(),
                kind: BadgeKind::Local,
                head: false,
                synced: false,
            });
        }
    }
    // Mark locals whose remote twin was folded in.
    for badge in &mut out {
        if badge.kind == BadgeKind::Local {
            badge.synced = decorations.iter().any(|d| {
                is_remote(d) && d.split_once('/').map(|(_, b)| b) == Some(badge.label.as_str())
            });
        }
    }
    // Checked-out branch first.
    out.sort_by_key(|b| !b.head);
    out
}

/// Paints a badge at `left_center`; returns its width.
fn ref_badge(
    ui: &Ui,
    painter: &egui::Painter,
    left_center: Pos2,
    badge: &Badge,
    lane: Color32,
    t: &Tokens,
) -> f32 {
    let (fill, text) = match badge.kind {
        BadgeKind::Local => (lane, Color32::from_rgb(16, 16, 16)),
        BadgeKind::Remote => (lane.gamma_multiply(0.25), t.text),
        BadgeKind::Tag => (t.field, t.text),
        BadgeKind::DetachedHead => (t.warning, Color32::from_rgb(16, 16, 16)),
    };
    let icon = match badge.kind {
        _ if badge.head => Some("check"),
        BadgeKind::Tag => Some("tag"),
        BadgeKind::Remote => Some("cloud"),
        _ => None,
    };
    let font = theme::medium(11.0);
    let galley = painter.layout_no_wrap(badge.label.clone(), font, text);
    let icon_w = if icon.is_some() { 14.0 } else { 0.0 };
    let synced_w = if badge.synced { 16.0 } else { 0.0 };
    let w = galley.size().x + 12.0 + icon_w + synced_w;
    let r = Rect::from_min_size(pos2(left_center.x, left_center.y - 9.0), vec2(w, 18.0));
    painter.rect_filled(r, CornerRadius::same(t.radius as u8), fill);
    if badge.kind == BadgeKind::Tag {
        painter.rect_stroke(
            r,
            CornerRadius::same(t.radius as u8),
            Stroke::new(1.0, t.field_border),
            egui::StrokeKind::Inside,
        );
    }
    let mut x = r.left() + 6.0;
    if let Some(icon) = icon {
        icons::paint(
            ui,
            Rect::from_center_size(pos2(x + 5.0, r.center().y), vec2(12.0, 12.0)),
            icon,
            11.0,
            text,
        );
        x += icon_w;
    }
    let gw = galley.size().x;
    painter.galley(pos2(x, r.center().y - galley.size().y / 2.0), galley, text);
    if badge.synced {
        icons::paint(
            ui,
            Rect::from_center_size(pos2(x + gw + 9.0, r.center().y), vec2(12.0, 12.0)),
            "cloud",
            11.0,
            text,
        );
    }
    w
}

/// A small rounded count label; returns its width.
fn pill(painter: &egui::Painter, left_center: Pos2, text: &str, color: Color32) -> f32 {
    let galley = painter.layout_no_wrap(text.to_owned(), theme::medium(11.0), color);
    let w = galley.size().x + 12.0;
    let r = Rect::from_min_size(pos2(left_center.x, left_center.y - 9.0), vec2(w, 18.0));
    painter.rect_filled(r, CornerRadius::same(9), color.gamma_multiply(0.18));
    painter.galley(
        pos2(r.left() + 6.0, r.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    w
}

fn circle_points(center: Pos2, radius: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            center + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

fn curve(p: &egui::Painter, from: Pos2, to: Pos2, color: Color32, width: f32) {
    if (from.x - to.x).abs() < 0.5 {
        p.line_segment([from, to], Stroke::new(width, color));
    } else {
        let midy = (from.y + to.y) / 2.0;
        p.add(CubicBezierShape::from_points_stroke(
            [from, pos2(from.x, midy), pos2(to.x, midy), to],
            false,
            Color32::TRANSPARENT,
            Stroke::new(width, color),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs_with_origin() -> Refs {
        Refs {
            remotes: vec![("origin".to_owned(), Vec::new())],
            ..Refs::default()
        }
    }

    #[test]
    fn remote_twin_folds_into_local_badge() {
        let decos = [
            "HEAD -> main",
            "origin/main",
            "origin/HEAD",
            "origin/dev",
            "tag: v1",
        ]
        .map(String::from);
        let b = badges(&decos, &refs_with_origin());
        let labels: Vec<&str> = b.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, vec!["main", "origin/dev", "v1"]);
        assert!(b[0].head && b[0].synced);
    }

    #[test]
    fn local_branch_with_slash_is_not_remote() {
        let decos = ["feature/x".to_owned()];
        let b = badges(&decos, &refs_with_origin());
        assert!(b[0].kind == BadgeKind::Local && !b[0].synced);
    }
}
