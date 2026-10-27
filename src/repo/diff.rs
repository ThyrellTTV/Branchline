//! Central pane while a file is open: its diff, unified or side by side, with syntax colours,
//! the changed part of each line marked, and images shown before / after.

use std::path::Path;
use std::sync::Arc;

use egui::{
    Align2, Color32, Rect, Sense, Stroke, Ui, load::Bytes, pos2, text::LayoutJob, text::TextFormat,
    vec2,
};

use super::highlight::{self, Spans};
use super::widgets;
use super::{Action, DiffKey, Refresh, RepoTab, ops};
use crate::git::{self, Area};
use crate::icons;
use crate::theme::{self, Tokens};

const LINE_H: f32 = 19.0;
const NUM_W: f32 = 46.0;
/// Very long diffs are cut here; the UI stays responsive and git is one click away.
const MAX_LINES: usize = 50_000;
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "ico"];

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

/// Everything the view needs, prepared on a background thread.
pub struct DiffData {
    pub lines: Vec<DiffLine>,
    /// Syntax colours per line (`None` when the file type isn't known).
    pub spans: Vec<Option<Spans>>,
    /// The changed part of a removed / added line paired with its counterpart (char range).
    pub changed: Vec<Option<(usize, usize)>>,
    /// Side-by-side rows: (left line, right line).
    pub split: Vec<(Option<usize>, Option<usize>)>,
    pub image: Option<ImagePair>,
}

pub struct ImagePair {
    pub old: Option<Arc<[u8]>>,
    pub new: Option<Arc<[u8]>>,
    /// Makes the image URIs unique to this diff.
    pub id: String,
}

/// Reads and prepares the diff for `key`.
pub fn load(repo: &Path, key: &DiffKey, ignore_ws: bool, dark: bool) -> Result<DiffData, String> {
    let (text, path) = match key {
        DiffKey::Commit { hash, path } => {
            (git::file_diff(repo, hash, path, ignore_ws)?, path.clone())
        }
        DiffKey::Work(file) => (git::work_diff(repo, file, ignore_ws)?, file.path.clone()),
    };
    let lines = parse(&text);
    let image = if is_image(&path) {
        Some(load_images(repo, key, &path))
    } else {
        None
    };

    // Colour the old and the new side each as one stream, so multi-line constructs work.
    let mut spans = vec![None; lines.len()];
    let old_side: Vec<usize> = (0..lines.len())
        .filter(|&i| matches!(lines[i].kind, Kind::Context | Kind::Del))
        .collect();
    let new_side: Vec<usize> = (0..lines.len())
        .filter(|&i| matches!(lines[i].kind, Kind::Context | Kind::Add))
        .collect();
    if let Some(old) = highlight::highlight(
        &path,
        old_side.iter().map(|&i| lines[i].text.as_str()),
        dark,
    ) {
        for (k, &i) in old_side.iter().enumerate() {
            if lines[i].kind == Kind::Del {
                spans[i] = old.get(k).cloned();
            }
        }
    }
    if let Some(new) = highlight::highlight(
        &path,
        new_side.iter().map(|&i| lines[i].text.as_str()),
        dark,
    ) {
        for (k, &i) in new_side.iter().enumerate() {
            spans[i] = new.get(k).cloned();
        }
    }

    let (changed, split) = pair_lines(&lines);
    Ok(DiffData {
        lines,
        spans,
        changed,
        split,
        image,
    })
}

fn is_image(path: &str) -> bool {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.as_str()))
}

fn load_images(repo: &Path, key: &DiffKey, path: &str) -> ImagePair {
    let get = |rev: Option<&str>| {
        git::file_at(repo, rev, path)
            .ok()
            .map(|b| Arc::from(b.into_boxed_slice()))
    };
    let (old, new, id) = match key {
        DiffKey::Commit { hash, .. } => (
            get(Some(&format!("{hash}^"))),
            get(Some(hash)),
            hash.clone(),
        ),
        DiffKey::Work(file) => match file.area {
            Area::Staged => (get(Some("HEAD")), get(Some("")), "staged".to_owned()),
            Area::Unstaged => (get(Some("")), get(None), "unstaged".to_owned()),
            Area::Untracked => (None, get(None), "untracked".to_owned()),
            Area::Conflicted => (None, get(None), "conflicted".to_owned()),
        },
    };
    ImagePair {
        old,
        new,
        id: format!("{id}-{}", git::now()),
    }
}

type Split = Vec<(Option<usize>, Option<usize>)>;

/// Pairs each run of removed lines with the added lines after it: marks what changed inside
/// each pair, and lays out side-by-side rows.
fn pair_lines(lines: &[DiffLine]) -> (Vec<Option<(usize, usize)>>, Split) {
    let mut changed = vec![None; lines.len()];
    let mut split = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        match lines[i].kind {
            Kind::Del | Kind::Add => {
                let dels_start = i;
                while i < lines.len() && lines[i].kind == Kind::Del {
                    i += 1;
                }
                let adds_start = i;
                while i < lines.len() && lines[i].kind == Kind::Add {
                    i += 1;
                }
                let dels: Vec<usize> = (dels_start..adds_start).collect();
                let adds: Vec<usize> = (adds_start..i).collect();
                for k in 0..dels.len().max(adds.len()) {
                    let (d, a) = (dels.get(k).copied(), adds.get(k).copied());
                    if let (Some(d), Some(a)) = (d, a)
                        && let Some((dr, ar)) = changed_ranges(&lines[d].text, &lines[a].text)
                    {
                        changed[d] = Some(dr);
                        changed[a] = Some(ar);
                    }
                    split.push((d, a));
                }
            }
            _ => {
                split.push((Some(i), Some(i)));
                i += 1;
            }
        }
    }
    (changed, split)
}

/// The differing middle of two lines (common prefix and suffix removed), as char ranges;
/// `None` when the lines have little in common (a rewrite, not an edit).
fn changed_ranges(old: &str, new: &str) -> Option<((usize, usize), (usize, usize))> {
    let a: Vec<char> = old.chars().collect();
    let b: Vec<char> = new.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let max_suffix = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x == y)
        .count();
    let common = prefix + suffix;
    if common * 3 < a.len().max(b.len()) {
        return None;
    }
    Some(((prefix, a.len() - suffix), (prefix, b.len() - suffix)))
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

        // Header: file, where it comes from, options, actions, close.
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
        let (split, ignore_ws) = (self.diff_split, self.diff_ignore_ws);
        egui::Frame::new()
            .fill(t.chrome)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .stroke(Stroke::new(1.0, t.separator))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    icons::inline(ui, "file-diff", 16.0, t.icon);
                    let title = ui.add(
                        egui::Label::new(egui::RichText::new(&path).font(theme::medium(13.0)))
                            .sense(Sense::click()),
                    );
                    widgets::file_menu(&title, &path, acts);
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
                        ui.add_space(8.0);
                        let ws = icons::small_button(
                            ui,
                            "pilcrow",
                            if ignore_ws {
                                "Show whitespace changes"
                            } else {
                                "Ignore whitespace changes"
                            },
                        );
                        if ignore_ws {
                            widgets::active_underline(ui, ws.rect, t.accent);
                        }
                        if ws.clicked() {
                            acts.push(Action::SetDiffOptions {
                                split,
                                ignore_ws: !ignore_ws,
                            });
                        }
                        let s = icons::small_button(ui, "columns-2", "Side by side");
                        if split {
                            widgets::active_underline(ui, s.rect, t.accent);
                        }
                        if s.clicked() {
                            acts.push(Action::SetDiffOptions {
                                split: true,
                                ignore_ws,
                            });
                        }
                        let u = icons::small_button(ui, "rows-2", "Unified");
                        if !split {
                            widgets::active_underline(ui, u.rect, t.accent);
                        }
                        if u.clicked() {
                            acts.push(Action::SetDiffOptions {
                                split: false,
                                ignore_ws,
                            });
                        }
                    });
                });
            });

        let data = match &view.data {
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
            Some(Ok(data)) => data,
        };
        if let Some(images) = &data.image {
            image_view(ui, images);
            return;
        }
        if data.lines.is_empty() {
            ui.centered_and_justified(|ui| {
                let msg = if ignore_ws {
                    "No changes besides whitespace"
                } else {
                    "No textual changes"
                };
                ui.label(egui::RichText::new(msg).color(t.text_dim));
            });
            return;
        }

        let font = theme::mono(12.0);
        let char_w = ui
            .painter()
            .layout_no_wrap("M".to_owned(), font.clone(), t.text)
            .size()
            .x;
        let longest = data
            .lines
            .iter()
            .map(|l| l.text.chars().count())
            .max()
            .unwrap_or(0) as f32;

        ui.spacing_mut().item_spacing.y = 0.0;
        let rows = if split {
            data.split.len()
        } else {
            data.lines.len()
        };
        egui::ScrollArea::both()
            .id_salt(("diff_scroll", &path, split))
            .auto_shrink(false)
            .show_rows(ui, LINE_H, rows, |ui, range| {
                if split {
                    let half =
                        (NUM_W + 24.0 + longest * char_w + 16.0).max(ui.available_width() / 2.0);
                    for row in range {
                        let Some(&(l, r)) = data.split.get(row) else {
                            continue;
                        };
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(half * 2.0, LINE_H), Sense::hover());
                        let left = Rect::from_min_size(rect.min, vec2(half, LINE_H));
                        let right = Rect::from_min_size(
                            pos2(rect.left() + half, rect.top()),
                            vec2(half, LINE_H),
                        );
                        // Hunk headers and notes span both halves.
                        if l == r
                            && let Some(i) = l
                            && matches!(
                                data.lines.get(i).map(|x| x.kind),
                                Some(Kind::Hunk | Kind::Meta)
                            )
                        {
                            paint_line(ui, rect, data, i, Side::Both, &font, &t);
                            continue;
                        }
                        match l {
                            Some(i) => paint_line(ui, left, data, i, Side::Old, &font, &t),
                            None => filler(ui, left, &t),
                        }
                        match r {
                            Some(i) => paint_line(ui, right, data, i, Side::New, &font, &t),
                            None => filler(ui, right, &t),
                        }
                        ui.painter().line_segment(
                            [right.left_top(), right.left_bottom()],
                            Stroke::new(1.0, t.separator),
                        );
                    }
                } else {
                    let width =
                        (NUM_W * 2.0 + 24.0 + longest * char_w + 24.0).max(ui.available_width());
                    for i in range {
                        let (rect, _) = ui.allocate_exact_size(vec2(width, LINE_H), Sense::hover());
                        paint_line(ui, rect, data, i, Side::Unified, &font, &t);
                    }
                }
            });
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Unified,
    Old,
    New,
    /// A hunk header across both halves of the split view.
    Both,
}

fn filler(ui: &Ui, rect: Rect, t: &Tokens) {
    ui.painter()
        .rect_filled(rect, 0.0, t.card.gamma_multiply(0.35));
}

fn paint_line(
    ui: &Ui,
    rect: Rect,
    data: &DiffData,
    i: usize,
    side: Side,
    font: &egui::FontId,
    t: &Tokens,
) {
    let Some(line) = data.lines.get(i) else {
        return;
    };
    let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
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
    let numbers: &[(Option<u32>, f32)] = match side {
        Side::Unified => &[(line.old, NUM_W - 8.0), (line.new, NUM_W * 2.0 - 8.0)],
        Side::Old => &[(line.old, NUM_W - 8.0)],
        Side::New => &[(line.new, NUM_W - 8.0)],
        Side::Both => &[],
    };
    let gutter = match side {
        Side::Unified => NUM_W * 2.0,
        Side::Both => 0.0,
        _ => NUM_W,
    };
    if gutter > 0.0 {
        p.rect_filled(
            Rect::from_min_size(rect.min, vec2(gutter, LINE_H)),
            0.0,
            t.card.gamma_multiply(0.6),
        );
    }
    for &(n, x) in numbers {
        if let Some(n) = n {
            p.text(
                pos2(rect.left() + x, y),
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
        pos2(rect.left() + gutter + 8.0, y),
        Align2::LEFT_CENTER,
        marker,
        font.clone(),
        mark_color,
    );

    // Text: syntax colours when known, the changed part on a stronger background.
    let strong = match line.kind {
        Kind::Add => t.success.gamma_multiply(0.35),
        Kind::Del => t.danger.gamma_multiply(0.35),
        _ => Color32::TRANSPARENT,
    };
    let changed = data.changed.get(i).copied().flatten();
    let spans: Vec<(Color32, &str)> = match (line.kind, data.spans.get(i).and_then(|s| s.as_ref()))
    {
        (Kind::Add | Kind::Del | Kind::Context, Some(spans)) => {
            spans.iter().map(|(c, s)| (*c, s.as_str())).collect()
        }
        _ => vec![(fg, line.text.as_str())],
    };
    let job = line_job(&spans, changed, font, strong);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let x = rect.left() + gutter + 24.0;
    p.galley(pos2(x, y - galley.size().y / 2.0), galley, fg);
}

/// A layout job for `spans`, with the chars in `changed` on `strong`.
fn line_job(
    spans: &[(Color32, &str)],
    changed: Option<(usize, usize)>,
    font: &egui::FontId,
    strong: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut pos = 0usize;
    for (color, text) in spans {
        let n = text.chars().count();
        let (start, end) = (pos, pos + n);
        // Split the span at the changed range's edges.
        let cuts: Vec<usize> = match changed {
            Some((a, b)) => vec![start, a.clamp(start, end), b.clamp(start, end), end],
            None => vec![start, end],
        };
        for w in cuts.windows(2) {
            let (from, to) = (w[0], w[1]);
            if to <= from {
                continue;
            }
            let piece: String = text.chars().skip(from - start).take(to - from).collect();
            let inside = changed.is_some_and(|(a, b)| from >= a && to <= b);
            job.append(
                &piece,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color: *color,
                    background: if inside { strong } else { Color32::TRANSPARENT },
                    ..Default::default()
                },
            );
        }
        pos = end;
    }
    job
}

fn image_view(ui: &mut Ui, images: &ImagePair) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(12.0);
    ui.columns(2, |cols| {
        for (col, (label, bytes, side)) in cols.iter_mut().zip([
            ("Before", &images.old, "old"),
            ("After", &images.new, "new"),
        ]) {
            col.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .font(theme::semibold(12.0))
                        .color(t.text_dim),
                );
                ui.add_space(6.0);
                match bytes {
                    Some(bytes) => {
                        let uri = format!("bytes://diff/{}/{side}.png", images.id);
                        let max = vec2(ui.available_width() - 16.0, ui.available_height() - 40.0);
                        ui.add(
                            egui::Image::from_bytes(uri, Bytes::Shared(bytes.clone()))
                                .max_size(max)
                                .fit_to_original_size(1.0)
                                .bg_fill(t.field),
                        );
                        ui.label(
                            egui::RichText::new(format!("{} KB", bytes.len().div_ceil(1024)))
                                .color(t.text_faint)
                                .small(),
                        );
                    }
                    None => {
                        let note = if side == "old" {
                            "(new file)"
                        } else {
                            "(deleted)"
                        };
                        ui.label(egui::RichText::new(note).color(t.text_faint));
                    }
                }
            });
        }
    });
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

    #[test]
    fn side_by_side_pairs_and_inline_changes() {
        let lines = parse("@@ -1,3 +1,3 @@\n a\n-let x = 1;\n-gone\n+let x = 2;\n b\n");
        let (changed, split) = pair_lines(&lines);
        // hunk, a, (del x1, add x2), (del gone, —), b
        assert_eq!(
            split,
            vec![
                (Some(0), Some(0)),
                (Some(1), Some(1)),
                (Some(2), Some(4)),
                (Some(3), None),
                (Some(5), Some(5))
            ]
        );
        assert_eq!(changed[2], Some((8, 9)));
        assert_eq!(changed[4], Some((8, 9)));
        assert_eq!(changed[3], None);
    }

    #[test]
    fn rewrites_are_not_marked_inline() {
        assert_eq!(changed_ranges("completely", "different words"), None);
        assert_eq!(changed_ranges("abc", "abXc"), Some(((2, 2), (2, 3))));
    }

    #[test]
    fn changed_part_gets_its_own_section() {
        let font = theme::mono(12.0);
        let job = line_job(
            &[(Color32::RED, "let x = 2;")],
            Some((8, 9)),
            &font,
            Color32::GREEN,
        );
        let pieces: Vec<&str> = job
            .sections
            .iter()
            .map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0])
            .collect();
        assert_eq!(pieces, vec!["let x = ", "2", ";"]);
        assert_eq!(job.sections[1].format.background, Color32::GREEN);
    }
}
