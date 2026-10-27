//! Right pane: the selected commit, or the working-tree changes with staging and the commit box.

use egui::{Align2, Key, RichText, Sense, Ui, pos2, vec2};

use super::diff::unstage_args;
use super::widgets::{self, RowExtra};
use super::{Action, Dialog, DiffKey, DiffView, Refresh, RepoTab, abort_action, ops};
use crate::git::{Area, StatusFile};
use crate::icons;
use crate::theme::{self, Tokens};

impl RepoTab {
    pub(super) fn detail_panel(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let wip = self.selected.is_some_and(|r| self.is_wip(r));
        if wip {
            self.changes_panel(ui, acts);
        } else {
            self.commit_panel(ui, acts);
        }
    }

    fn open_key(&self) -> Option<&DiffKey> {
        self.diff.as_ref().map(|d: &DiffView| &d.key)
    }

    fn commit_panel(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        widgets::panel_header(ui, "Commit", |_| {});
        if let Some(err) = &self.detail_error {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.colored_label(t.danger, err);
            });
            return;
        }
        let Some(detail) = &self.detail else {
            ui.centered_and_justified(|ui| {
                if self.selected.is_some() {
                    ui.spinner();
                } else {
                    ui.label(RichText::new("Select a commit").color(t.text_dim));
                }
            });
            return;
        };

        egui::ScrollArea::vertical()
            .id_salt("detail_scroll")
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(14, 12))
                    .show(ui, |ui| {
                        let (subject, body) = detail
                            .message
                            .split_once('\n')
                            .map(|(s, b)| (s, b.trim()))
                            .unwrap_or((detail.message.as_str(), ""));
                        ui.add(
                            egui::Label::new(RichText::new(subject).font(theme::semibold(15.0)))
                                .wrap(),
                        );
                        if !body.is_empty() {
                            ui.add_space(2.0);
                            ui.add(egui::Label::new(RichText::new(body).color(t.text_dim)).wrap());
                        }
                        ui.add_space(12.0);

                        // Author line with an initials avatar.
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                            widgets::avatar(ui.painter(), rect.center(), 16.0, &detail.author);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(&detail.author).font(theme::medium(12.5)))
                                    .on_hover_text(&detail.email);
                                ui.label(
                                    RichText::new(&detail.author_date)
                                        .color(t.text_faint)
                                        .font(theme::mono(11.0)),
                                );
                            });
                        });
                        if detail.committer != detail.author {
                            ui.label(
                                RichText::new(format!(
                                    "Committed by {} · {}",
                                    detail.committer, detail.commit_date
                                ))
                                .color(t.text_faint)
                                .small(),
                            );
                        }
                        ui.add_space(10.0);

                        egui::Grid::new("commit_meta")
                            .num_columns(2)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new("Commit").color(t.text_faint));
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(
                                            detail.hash.get(..12).unwrap_or(&detail.hash),
                                        )
                                        .font(theme::mono(12.0)),
                                    )
                                    .on_hover_text(&detail.hash);
                                    if icons::small_button(ui, "copy", "Copy full hash").clicked() {
                                        ui.ctx().copy_text(detail.hash.clone());
                                    }
                                });
                                ui.end_row();
                                if !detail.parents.is_empty() {
                                    ui.label(
                                        RichText::new(if detail.parents.len() > 1 {
                                            "Parents"
                                        } else {
                                            "Parent"
                                        })
                                        .color(t.text_faint),
                                    );
                                    ui.horizontal_wrapped(|ui| {
                                        for p in &detail.parents {
                                            if ui
                                                .link(
                                                    RichText::new(p.get(..8).unwrap_or(p))
                                                        .font(theme::mono(12.0)),
                                                )
                                                .clicked()
                                            {
                                                acts.push(Action::SelectHash(p.clone()));
                                            }
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });

                let (adds, dels) = detail.files.iter().fold((0, 0), |(a, d), f| {
                    (a + f.added.unwrap_or(0), d + f.removed.unwrap_or(0))
                });
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(rect, 0.0, t.tab_strip);
                let y = rect.center().y;
                let r = p.text(
                    pos2(rect.left() + 14.0, y),
                    Align2::LEFT_CENTER,
                    format!(
                        "{} file{} changed",
                        detail.files.len(),
                        if detail.files.len() == 1 { "" } else { "s" }
                    ),
                    theme::semibold(11.5),
                    t.text_dim,
                );
                let r = p.text(
                    pos2(r.right() + 10.0, y),
                    Align2::LEFT_CENTER,
                    format!("+{adds}"),
                    theme::mono(11.0),
                    t.success,
                );
                p.text(
                    pos2(r.right() + 6.0, y),
                    Align2::LEFT_CENTER,
                    format!("−{dels}"),
                    theme::mono(11.0),
                    t.danger,
                );
                if detail.parents.len() > 1 {
                    p.text(
                        pos2(rect.right() - 10.0, y),
                        Align2::RIGHT_CENTER,
                        "vs. first parent",
                        egui::FontId::proportional(11.0),
                        t.text_faint,
                    );
                }

                ui.spacing_mut().item_spacing.y = 0.0;
                for f in &detail.files {
                    let key = DiffKey::Commit {
                        hash: detail.hash.clone(),
                        path: f.path.clone(),
                    };
                    let selected = self.open_key() == Some(&key);
                    let r = widgets::file_row(
                        ui,
                        f.status,
                        &f.path,
                        f.old_path.as_deref(),
                        selected,
                        RowExtra::Stats(f.added, f.removed),
                    );
                    if r.row.clicked() {
                        acts.push(Action::OpenDiff(key));
                    }
                }
            });
    }

    fn changes_panel(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let idle = self.busy_op.is_none();
        widgets::panel_header(ui, "Changes", |_| {});

        // The commit box stays at the bottom.
        egui::Panel::bottom("commit_box")
            .frame(
                egui::Frame::new()
                    .fill(t.card)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .stroke(egui::Stroke::new(1.0, t.separator)),
            )
            .show(ui, |ui| self.commit_box(ui, acts));

        egui::ScrollArea::vertical()
            .id_salt("changes_scroll")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if let Some(state) = self.work.in_progress {
                    egui::Frame::new()
                        .fill(t.warning.gamma_multiply(0.14))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                icons::inline(ui, "circle-alert", 16.0, t.warning);
                                ui.label(
                                    RichText::new(format!("{} in progress", state.label()))
                                        .color(t.warning)
                                        .font(theme::medium(12.5)),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_enabled(idle, egui::Button::new("Abort"))
                                            .clicked()
                                        {
                                            acts.push(abort_action(state));
                                        }
                                    },
                                );
                            });
                            let hint = if self.work.conflicted.is_empty() {
                                "All conflicts resolved: commit to finish."
                            } else {
                                "Fix the conflicted files, mark them resolved, then commit."
                            };
                            ui.label(RichText::new(hint).color(t.text_dim).small());
                        });
                }

                let head = self.refs.head.clone();
                if !self.work.conflicted.is_empty() {
                    list_header(ui, "CONFLICTED", self.work.conflicted.len(), None);
                    for f in &self.work.conflicted {
                        match self.work_row(ui, f, &[("check", "Mark resolved (stage)")]) {
                            RowClick::Open => acts.push(Action::OpenDiff(DiffKey::Work(f.clone()))),
                            RowClick::Button(_) => acts.push(stage_action(
                                "Mark resolved",
                                vec![vec!["add", "--", &f.path]],
                            )),
                            RowClick::None => {}
                        }
                    }
                }

                let unstaged = &self.work.unstaged;
                if list_header(
                    ui,
                    "UNSTAGED",
                    unstaged.len(),
                    (!unstaged.is_empty() && idle).then_some("Stage all"),
                ) {
                    acts.push(stage_action("Stage all", vec![vec!["add", "-A"]]));
                }
                for f in unstaged {
                    match self.work_row(ui, f, &[("undo-2", "Discard changes…"), ("plus", "Stage")])
                    {
                        RowClick::Open => acts.push(Action::OpenDiff(DiffKey::Work(f.clone()))),
                        RowClick::Button(0) => acts.push(Action::Dialog(discard_dialog(f))),
                        RowClick::Button(_) => {
                            acts.push(stage_action("Stage", vec![vec!["add", "--", &f.path]]))
                        }
                        RowClick::None => {}
                    }
                }
                if unstaged.is_empty() {
                    empty_hint(ui, "Nothing to stage");
                }

                let staged = &self.work.staged;
                if list_header(
                    ui,
                    "STAGED",
                    staged.len(),
                    (!staged.is_empty() && idle).then_some("Unstage all"),
                ) {
                    let all: Vec<&str> = if head.is_empty() {
                        vec!["rm", "--cached", "-r", "-q", "--", "."]
                    } else {
                        vec!["reset", "-q"]
                    };
                    acts.push(stage_action("Unstage all", vec![all]));
                }
                for f in staged {
                    match self.work_row(ui, f, &[("minus", "Unstage")]) {
                        RowClick::Open => acts.push(Action::OpenDiff(DiffKey::Work(f.clone()))),
                        RowClick::Button(_) => {
                            acts.push(stage_action("Unstage", vec![unstage_args(&head, &f.path)]))
                        }
                        RowClick::None => {}
                    }
                }
                if staged.is_empty() {
                    empty_hint(ui, "Stage files to choose what goes in the commit");
                }
            });
    }

    /// One working-tree file with hover buttons (hidden while an operation runs).
    fn work_row(&self, ui: &mut Ui, f: &StatusFile, buttons: &[(&str, &str)]) -> RowClick {
        let key = DiffKey::Work(f.clone());
        let selected = self.open_key() == Some(&key);
        let buttons = if self.busy_op.is_none() { buttons } else { &[] };
        let r = widgets::file_row(
            ui,
            f.code,
            &f.path,
            f.old_path.as_deref(),
            selected,
            RowExtra::Buttons(buttons),
        );
        match r.button {
            Some(i) => RowClick::Button(i),
            None if r.row.clicked() => RowClick::Open,
            None => RowClick::None,
        }
    }

    fn commit_box(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let idle = self.busy_op.is_none();
        ui.label(
            RichText::new("COMMIT")
                .font(theme::semibold(10.5))
                .color(t.text_faint),
        );
        ui.add_space(2.0);
        let summary = ui.add(
            egui::TextEdit::singleline(&mut self.commit_summary)
                .hint_text("Summary")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(8, 6)),
        );
        let body = ui.add(
            egui::TextEdit::multiline(&mut self.commit_body)
                .hint_text("Description (optional)")
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .margin(egui::Margin::symmetric(8, 6)),
        );
        let len = self.commit_summary.chars().count();
        if len > 72 {
            ui.label(
                RichText::new(format!("Summary is {len} characters; 50–72 reads best"))
                    .color(t.warning)
                    .small(),
            );
        }
        ui.add_space(4.0);
        let staged = self.work.staged.len();
        let label = if staged > 0 {
            format!("Commit {staged} file{}", if staged == 1 { "" } else { "s" })
        } else {
            "Stage all and commit".to_owned()
        };
        let can = idle && !self.commit_summary.trim().is_empty() && self.work.conflicted.is_empty();
        // Only from the message fields, so Ctrl+Enter in the terminal stays the shell's.
        let shortcut = (summary.has_focus() || body.has_focus())
            && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
        let resp = widgets::primary_button(ui, &label, can, ui.available_width());
        if (resp.clicked() || (shortcut && can)) && can {
            acts.push(Action::Commit);
        }
        resp.on_hover_text("Ctrl+Enter");
    }
}

enum RowClick {
    None,
    Open,
    Button(usize),
}

fn stage_action(label: &str, steps: Vec<Vec<&str>>) -> Action {
    Action::Ops {
        label: label.to_owned(),
        ops: steps
            .into_iter()
            .map(|s| s.into_iter().map(str::to_owned).collect())
            .collect(),
        refresh: Refresh::Status,
    }
}

fn discard_dialog(f: &StatusFile) -> Dialog {
    if f.area == Area::Untracked {
        Dialog::confirm(
            "Delete new file",
            format!(
                "Delete “{}”? It isn't tracked by git, so this can't be undone.",
                f.path
            ),
            "Delete file",
            ops(&[&["clean", "-f", "-q", "--", &f.path]]),
            None,
        )
        .refresh(Refresh::Status)
    } else {
        Dialog::confirm(
            "Discard changes",
            format!(
                "Discard the unstaged changes to “{}”? This can't be undone.",
                f.path
            ),
            "Discard",
            ops(&[&["restore", "--", &f.path]]),
            None,
        )
        .refresh(Refresh::Status)
    }
}

/// A list title with a count and an optional text button on the right. Returns the click.
fn list_header(ui: &mut Ui, title: &str, count: usize, button: Option<&str>) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.tab_strip);
    let y = rect.center().y;
    let r = p.text(
        pos2(rect.left() + 12.0, y),
        Align2::LEFT_CENTER,
        title,
        theme::semibold(11.0),
        t.text_dim,
    );
    p.text(
        pos2(r.right() + 8.0, y),
        Align2::LEFT_CENTER,
        count.to_string(),
        theme::medium(11.0),
        t.text_faint,
    );
    let Some(label) = button else { return false };
    let galley = p.layout_no_wrap(label.to_owned(), theme::medium(11.5), t.accent);
    let b = egui::Rect::from_center_size(
        pos2(rect.right() - 12.0 - galley.size().x / 2.0 - 6.0, y),
        vec2(galley.size().x + 12.0, 22.0),
    );
    let resp = ui.interact(b, ui.id().with(("list_btn", title)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(b, t.radius_sm, t.hover);
    }
    ui.painter().galley(
        pos2(b.left() + 6.0, y - galley.size().y / 2.0),
        galley,
        t.accent,
    );
    resp.clicked()
}

fn empty_hint(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(11.5),
        t.text_faint,
    );
}
