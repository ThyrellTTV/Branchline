//! Central pane while a file is open: its diff with old/new line numbers.

use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::widgets;
use super::{Action, DiffKey, Refresh, RepoTab, ops};
use crate::git::Area;
use crate::icons;
use crate::theme::{self, Tokens};

const LINE_H: f32 = 19.0;
const NUM_W: f32 = 46.0;
/// Very long diffs are cut here; the UI stays responsive and git is one click away.
const MAX_LINES: usize = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Context,
    Add,
    Del,
    Hunk,
    /// Binary notices, "No newline at end of file".
    Meta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: Kind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

/// Parses a unified (or combined, for conflicts) diff of one file.
pub fn parse(text: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    let mut in_hunk = false;
    // Combined diffs (`@@@`) prefix every line with one column per parent.
    let mut prefix = 1;
    for line in text.lines() {
        if out.len() >= MAX_LINES {
            out.push(meta("… diff truncated"));
            break;
        }
        if line.starts_with("@@") {
            in_hunk = true;
            let ats = line.chars().take_while(|c| *c == '@').count();
            prefix = ats.saturating_sub(1).max(1);
            (old, new) = hunk_start(line);
            out.push(DiffLine {
                kind: Kind::Hunk,
                old: None,
                new: None,
                text: line.to_owned(),
            });
            continue;
        }
        if !in_hunk {
            if line.starts_with("Binary files") {
                out.push(meta(line));
            }
            continue;
        }
        if line.starts_with('\\') {
            out.push(meta(line));
            continue;
        }
        if line.starts_with("diff ") {
            // Next file (shouldn't happen for a single path): back to the preamble.
            in_hunk = false;
            continue;
        }
        let cut = line
            .char_indices()
            .nth(prefix)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        let (marks, body) = line.split_at(cut);
        let kind = if marks.contains('+') {
            Kind::Add
        } else if marks.contains('-') {
            Kind::Del
        } else {
            Kind::Context
        };
        let combined = prefix > 1;
        let (o, n) = match kind {
            Kind::Add => {
                new += 1;
                (None, Some(new))
            }
            Kind::Del => {
                old += 1;
                (Some(old), None)
            }
            _ => {
                old += 1;
                new += 1;
                (Some(old), Some(new))
            }
        };
        out.push(DiffLine {
            kind,
            old: if combined { None } else { o },
            new: if combined { None } else { n },
            text: body.replace('\t', "    "),
        });
    }
    out
}

fn meta(text: &str) -> DiffLine {
    DiffLine {
        kind: Kind::Meta,
        old: None,
        new: None,
        text: text.to_owned(),
    }
}

/// `@@ -12,5 +14,7 @@` → (11, 13): the numbers before the first line of the hunk.
fn hunk_start(header: &str) -> (u32, u32) {
    let mut old = 0;
    let mut new = 0;
    for part in header.split_whitespace() {
        let num = |s: &str| {
            s.split(',')
                .next()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(1)
                .saturating_sub(1)
        };
        if let Some(rest) = part.strip_prefix('-') {
            old = num(rest);
        } else if let Some(rest) = part.strip_prefix('+') {
            new = num(rest);
        }
    }
    (old, new)
}

impl RepoTab {
    pub(super) fn diff_view(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let Some(view) = &self.diff else { return };
        let idle = self.busy_op.is_none();

        // Header: file, where it comes from, actions, close.
        let (path, origin) = match &view.key {
            DiffKey::Commit { hash, path } => (
                path.clone(),
                format!("Commit {}", hash.get(..8).unwrap_or(hash)),
            ),
            DiffKey::Work(file) => (
                file.path.clone(),
                match file.area {
                    Area::Staged => "Staged changes".to_owned(),
                    Area::Unstaged => "Unstaged changes".to_owned(),
                    Area::Untracked => "New file".to_owned(),
                    Area::Conflicted => "Conflicted".to_owned(),
                },
            ),
        };
        egui::Frame::new()
            .fill(t.chrome)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .stroke(Stroke::new(1.0, t.separator))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    icons::inline(ui, "file-diff", 16.0, t.icon);
                    ui.label(egui::RichText::new(&path).font(theme::medium(13.0)));
                    widgets::chip(ui, &origin, t.text_dim);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::small_button(ui, "x", "Close (Esc)").clicked() {
                            acts.push(Action::CloseDiff);
                        }
                        if let DiffKey::Work(file) = &view.key {
                            let (label, args): (&str, Vec<&str>) = match file.area {
                                Area::Staged => {
                                    ("Unstage file", unstage_args(&self.refs.head, &file.path))
                                }
                                Area::Conflicted => {
                                    ("Mark resolved", vec!["add", "--", &file.path])
                                }
                                _ => ("Stage file", vec!["add", "--", &file.path]),
                            };
                            if ui.add_enabled(idle, egui::Button::new(label)).clicked() {
                                acts.push(Action::Ops {
                                    label: label.to_owned(),
                                    ops: ops(&[&args]),
                                    refresh: Refresh::Status,
                                });
                            }
                        }
                    });
                });
            });

        let lines = match &view.lines {
            None => {
                ui.centered_and_justified(|ui| ui.spinner());
                return;
            }
            Some(Err(e)) => {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.colored_label(t.danger, e);
                });
                return;
            }
            Some(Ok(lines)) => lines,
        };
        if lines.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No textual changes").color(t.text_dim));
            });
            return;
        }

        let font = theme::mono(12.0);
        let char_w = ui
            .painter()
            .layout_no_wrap("M".to_owned(), font.clone(), t.text)
            .size()
            .x;
        let longest = lines
            .iter()
            .map(|l| l.text.chars().count())
            .max()
            .unwrap_or(0);
        let content_w = NUM_W * 2.0 + 24.0 + longest as f32 * char_w + 24.0;

        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::both()
            .id_salt(("diff_scroll", &path))
            .auto_shrink(false)
            .show_rows(ui, LINE_H, lines.len(), |ui, range| {
                let width = content_w.max(ui.available_width());
                for line in lines.get(range).unwrap_or_default() {
                    let (rect, _) = ui.allocate_exact_size(vec2(width, LINE_H), Sense::hover());
                    let p = ui.painter();
                    let (bg, fg) = match line.kind {
                        Kind::Add => (Some(t.diff_add_bg), t.text),
                        Kind::Del => (Some(t.diff_del_bg), t.text),
                        Kind::Hunk => (Some(t.accent.gamma_multiply(0.12)), t.accent),
                        Kind::Meta => (None, t.text_faint),
                        Kind::Context => (None, t.text_dim),
                    };
                    if let Some(bg) = bg {
                        p.rect_filled(rect, 0.0, bg);
                    }
                    let y = rect.center().y;
                    let gutter = Rect::from_min_size(rect.min, vec2(NUM_W * 2.0, LINE_H));
                    p.rect_filled(gutter, 0.0, t.card.gamma_multiply(0.6));
                    for (n, x) in [
                        (line.old, rect.left() + NUM_W - 8.0),
                        (line.new, rect.left() + NUM_W * 2.0 - 8.0),
                    ] {
                        if let Some(n) = n {
                            p.text(
                                pos2(x, y),
                                Align2::RIGHT_CENTER,
                                n.to_string(),
                                theme::mono(11.0),
                                t.text_faint,
                            );
                        }
                    }
                    let marker = match line.kind {
                        Kind::Add => "+",
                        Kind::Del => "−",
                        _ => "",
                    };
                    let mark_color = if line.kind == Kind::Add {
                        t.success
                    } else {
                        t.danger
                    };
                    p.text(
                        pos2(rect.left() + NUM_W * 2.0 + 8.0, y),
                        Align2::LEFT_CENTER,
                        marker,
                        font.clone(),
                        mark_color,
                    );
                    p.text(
                        pos2(rect.left() + NUM_W * 2.0 + 24.0, y),
                        Align2::LEFT_CENTER,
                        &line.text,
                        font.clone(),
                        fg,
                    );
                }
            });
    }
}

/// `git restore --staged`, or `git rm --cached` before the first commit (no HEAD to restore from).
pub(super) fn unstage_args<'a>(head: &str, path: &'a str) -> Vec<&'a str> {
    if head.is_empty() {
        vec!["rm", "--cached", "-q", "--", path]
    } else {
        vec!["restore", "--staged", "--", path]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_hunk_headers() {
        let text = "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -3,3 +3,4 @@ fn x\n a\n-b\n+c\n+d\n e\n\\ No newline at end of file\n";
        let lines = parse(text);
        let summary: Vec<_> = lines.iter().map(|l| (l.kind, l.old, l.new)).collect();
        assert_eq!(
            summary,
            vec![
                (Kind::Hunk, None, None),
                (Kind::Context, Some(3), Some(3)),
                (Kind::Del, Some(4), None),
                (Kind::Add, None, Some(4)),
                (Kind::Add, None, Some(5)),
                (Kind::Context, Some(5), Some(6)),
                (Kind::Meta, None, None),
            ]
        );
        assert_eq!(lines[3].text, "c");
    }

    #[test]
    fn combined_conflict_diff() {
        let text = "@@@ -1,1 -1,1 +1,5 @@@\n++<<<<<<< HEAD\n +ours\n  same\n";
        let lines = parse(text);
        assert_eq!(lines[1].kind, Kind::Add);
        assert_eq!(lines[1].text, "<<<<<<< HEAD");
        assert_eq!(lines[3].kind, Kind::Context);
        assert_eq!(lines[3].old, None);
    }

    #[test]
    fn binary_notice() {
        let lines = parse("diff --git a/x b/x\nBinary files a/x and b/x differ\n");
        assert_eq!(lines, vec![meta("Binary files a/x and b/x differ")]);
    }
}
