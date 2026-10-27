//! Central pane: the commit graph of every branch.

use egui::{
    Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, Response, Sense, Stroke, Ui,
    epaint::CubicBezierShape, pos2, vec2,
};

use super::widgets;
use super::{Action, Dialog, Refresh, RepoTab, WIP_HASH, ops};
use crate::git::Refs;
use crate::icons;
use crate::theme::{self, Tokens};

const ROW_H: f32 = 28.0;
const LANE_W: f32 = 16.0;
const MAX_LANES_SHOWN: usize = 24;
const AUTHOR_W: f32 = 150.0;
const DATE_W: f32 = 118.0;
/// Width of the branch/tag column left of the graph.
const REFS_W: f32 = 210.0;
const AVATAR_D: f32 = 20.0;

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
                (header.left() + 10.0, "BRANCH / TAG"),
                (header.left() + REFS_W + 10.0, "GRAPH"),
                (header.left() + REFS_W + graph_w + 6.0, "DESCRIPTION"),
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

        let (top, mid, bottom) = (rect.top(), rect.center().y, rect.bottom());
        let gx = rect.left() + REFS_W;
        let x = |lane: usize| gx + 6.0 + lane as f32 * LANE_W + LANE_W / 2.0;
        let row_color = self
            .graph
            .rows
            .get(i)
            .map(|r| t.lane(r.col))
            .unwrap_or(t.text_faint);
        let node_x = self.graph.rows.get(i).map(|r| x(r.col));

        // Branch and tag pills, left of the graph, joined to the commit's dot.
        let badges = badges(&commit.refs, &self.refs);
        let mut resp = resp;
        if let (Some(first), Some(node_x)) = (badges.first(), node_x) {
            let refs_clip = Rect::from_min_max(rect.min, pos2(gx - 4.0, bottom));
            let rp = painter.with_clip_rect(refs_clip);
            let more = badges.len() - 1;
            let more_w = if more > 0 { 28.0 } else { 0.0 };
            let max_w = REFS_W - 10.0 - AVATAR_D - 8.0 - more_w - 8.0;
            let pill = ref_pill(
                ui,
                &rp,
                pos2(rect.left() + 8.0, mid),
                first,
                row_color,
                max_w,
            );
            let mut end = pill.right();
            if more > 0 {
                let r = Rect::from_min_size(pos2(end + 3.0, mid - 10.0), vec2(more_w - 3.0, 20.0));
                rp.rect_filled(
                    r,
                    CornerRadius::same(t.radius as u8),
                    row_color.lerp_to_gamma(Color32::BLACK, 0.35),
                );
                rp.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    format!("+{more}"),
                    theme::medium(10.5),
                    Color32::WHITE,
                );
                end = r.right();
            }
            // Avatar of whoever made the tip commit.
            let avatar = pos2(end + 5.0 + AVATAR_D / 2.0, mid);
            // Connector to the dot, drawn first so the lanes cross over it.
            painter.line_segment(
                [pos2(avatar.x, mid), pos2(node_x, mid)],
                Stroke::new(1.5, row_color.gamma_multiply(0.8)),
            );
            widgets::avatar(&rp, avatar, AVATAR_D / 2.0, &commit.author);
            let refs_cell = Rect::from_min_max(rect.min, pos2(gx, bottom));
            if ui.rect_contains_pointer(refs_cell) {
                let lines: Vec<String> = badges.iter().map(Badge::describe).collect();
                resp = resp.on_hover_text(format!(
                    "{}\nTip commit by {}",
                    lines.join("\n"),
                    commit.author
                ));
            }
        }

        // Graph.
        if let Some(row) = self.graph.rows.get(i) {
            let gp = painter.with_clip_rect(
                rect.intersect(Rect::from_min_size(pos2(gx, top), vec2(graph_w, ROW_H))),
            );
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

        let mut x = gx + graph_w + 6.0;
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

/// One pill in the refs column: a branch (local, remote or both), a tag or a detached HEAD.
#[derive(Debug)]
struct Badge {
    label: String,
    kind: BadgeKind,
    /// The branch exists locally.
    local: bool,
    /// Remotes that have the branch at this commit.
    remotes: Vec<String>,
    /// The checked-out branch (or a detached HEAD).
    head: bool,
}

#[derive(Debug, PartialEq)]
enum BadgeKind {
    Branch,
    Tag,
    DetachedHead,
}

impl Badge {
    fn describe(&self) -> String {
        match self.kind {
            BadgeKind::Tag => format!("tag {}", self.label),
            BadgeKind::DetachedHead => "HEAD (detached)".to_owned(),
            BadgeKind::Branch => {
                let mut places = Vec::new();
                if self.local {
                    places.push(if self.head {
                        "local, checked out".to_owned()
                    } else {
                        "local".to_owned()
                    });
                }
                places.extend(self.remotes.iter().cloned());
                format!("{} — {}", self.label, places.join(", "))
            }
        }
    }
}

/// Turns `git log --format=%D` decorations into pills: `x` and `origin/x` at the same commit
/// become one pill marked local and remote.
fn badges(decorations: &[String], refs: &Refs) -> Vec<Badge> {
    let remote_of = |d: &str| {
        refs.remotes.iter().find_map(|(remote, _)| {
            d.strip_prefix(remote.as_str())
                .and_then(|r| r.strip_prefix('/'))
                .map(|branch| (remote.clone(), branch.to_owned()))
        })
    };
    fn branch(out: &mut Vec<Badge>, name: &str) -> usize {
        match out
            .iter()
            .position(|b| b.kind == BadgeKind::Branch && b.label == name)
        {
            Some(i) => i,
            None => {
                out.push(Badge {
                    label: name.to_owned(),
                    kind: BadgeKind::Branch,
                    local: false,
                    remotes: Vec::new(),
                    head: false,
                });
                out.len() - 1
            }
        }
    }
    let mut out: Vec<Badge> = Vec::new();
    for d in decorations {
        if let Some(name) = d.strip_prefix("HEAD -> ") {
            let i = branch(&mut out, name);
            if let Some(b) = out.get_mut(i) {
                b.local = true;
                b.head = true;
            }
        } else if d == "HEAD" {
            out.push(Badge {
                label: "HEAD".to_owned(),
                kind: BadgeKind::DetachedHead,
                local: false,
                remotes: Vec::new(),
                head: true,
            });
        } else if let Some(tag) = d.strip_prefix("tag: ") {
            out.push(Badge {
                label: tag.to_owned(),
                kind: BadgeKind::Tag,
                local: false,
                remotes: Vec::new(),
                head: false,
            });
        } else if let Some((remote, name)) = remote_of(d) {
            // `origin/HEAD` only says which branch the remote defaults to.
            if name == "HEAD" {
                continue;
            }
            let i = branch(&mut out, &name);
            if let Some(b) = out.get_mut(i) {
                b.remotes.push(remote);
            }
        } else {
            let i = branch(&mut out, d);
            if let Some(b) = out.get_mut(i) {
                b.local = true;
            }
        }
    }
    // Checked out first, then local branches, remote-only ones, tags.
    out.sort_by_key(|b| match b.kind {
        _ if b.head => 0,
        BadgeKind::Branch if b.local => 1,
        BadgeKind::Branch => 2,
        BadgeKind::Tag => 3,
        BadgeKind::DetachedHead => 4,
    });
    out
}

/// Paints a pill with white text and icons, at most `max_w` wide; returns its rect.
fn ref_pill(
    ui: &Ui,
    painter: &egui::Painter,
    left_center: Pos2,
    badge: &Badge,
    lane: Color32,
    max_w: f32,
) -> Rect {
    const ICON: f32 = 15.0;
    let t = Tokens::get(ui.ctx());
    let white = Color32::WHITE;
    let fill = match badge.kind {
        BadgeKind::Branch if badge.local => lane,
        // Only on a remote: a darker shade of the lane.
        BadgeKind::Branch => lane.lerp_to_gamma(Color32::BLACK, 0.35),
        BadgeKind::Tag => Color32::from_rgb(104, 104, 112),
        BadgeKind::DetachedHead => t.warning,
    };
    let mut lead = Vec::new();
    let mut trail = Vec::new();
    match badge.kind {
        BadgeKind::Tag => lead.push("tag"),
        BadgeKind::DetachedHead => lead.push("check"),
        BadgeKind::Branch => {
            if badge.head {
                lead.push("check");
            }
            // Where the branch lives: this computer, a remote, or both.
            if badge.local {
                trail.push("laptop");
            }
            if !badge.remotes.is_empty() {
                trail.push("cloud");
            }
        }
    }
    let font = if badge.head {
        theme::semibold(11.0)
    } else {
        theme::medium(11.0)
    };
    let icons_w = (lead.len() + trail.len()) as f32 * ICON;
    let text_room = (max_w - 16.0 - icons_w).max(16.0);
    let label = truncate(painter, &badge.label, &font, text_room);
    let galley = painter.layout_no_wrap(label, font, white);
    let w = (galley.size().x + 16.0 + icons_w).min(max_w);
    let r = Rect::from_min_size(pos2(left_center.x, left_center.y - 10.0), vec2(w, 20.0));
    painter.rect_filled(r, CornerRadius::same(t.radius as u8), fill);
    let mut x = r.left() + 7.0;
    for icon in lead {
        icons::paint(
            ui,
            Rect::from_center_size(pos2(x + 6.0, r.center().y), vec2(12.0, 12.0)),
            icon,
            11.0,
            white,
        );
        x += ICON;
    }
    let gw = galley.size().x;
    painter.galley(pos2(x, r.center().y - galley.size().y / 2.0), galley, white);
    x += gw + 1.0;
    for icon in trail {
        icons::paint(
            ui,
            Rect::from_center_size(pos2(x + 8.0, r.center().y), vec2(12.0, 12.0)),
            icon,
            11.0,
            white,
        );
        x += ICON;
    }
    r
}

/// `text`, cut with an ellipsis to fit `width`.
fn truncate(painter: &egui::Painter, text: &str, font: &FontId, width: f32) -> String {
    let fits = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len();
    while n > 1 {
        n -= 1;
        let s: String = chars.iter().take(n).collect::<String>() + "…";
        if fits(&s) {
            return s;
        }
    }
    "…".to_owned()
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
            "origin/main",
            "HEAD -> main",
            "origin/HEAD",
            "origin/dev",
            "tag: v1",
        ]
        .map(String::from);
        let b = badges(&decos, &refs_with_origin());
        let labels: Vec<&str> = b.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, vec!["main", "dev", "v1"]);
        assert!(b[0].head && b[0].local && b[0].remotes == ["origin"]);
        assert!(!b[1].local && b[1].remotes == ["origin"]);
        assert_eq!(b[2].kind, BadgeKind::Tag);
    }

    #[test]
    fn local_branch_with_slash_is_not_remote() {
        let decos = ["feature/x".to_owned()];
        let b = badges(&decos, &refs_with_origin());
        assert!(b[0].kind == BadgeKind::Branch && b[0].local && b[0].remotes.is_empty());
    }
}
