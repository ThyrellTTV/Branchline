//! Left pane: local branches, remote branches per remote, tags.

use egui::{Align2, Rect, Response, Sense, Ui, pos2, vec2};

use super::widgets::{self, ROW_H};
use super::{Action, Dialog, Refresh, RepoTab, ops};
use crate::git::Branch;
use crate::icons;
use crate::theme::{self, Tokens};

impl RepoTab {
    pub(super) fn sidebar(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        widgets::panel_header(ui, "Branches", |_| {});
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            icons::inline(ui, "search", 14.0, t.text_faint);
            ui.add(
                egui::TextEdit::singleline(&mut self.branch_filter)
                    .hint_text("Filter")
                    .desired_width(ui.available_width() - 8.0),
            );
        });
        ui.add_space(4.0);

        let filter = self.branch_filter.to_lowercase();
        let matches = |b: &&Branch| filter.is_empty() || b.name.to_lowercase().contains(&filter);
        let current = self.refs.current.clone();
        let idle = self.busy_op.is_none();

        egui::ScrollArea::vertical()
            .id_salt("branches")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;

                let local: Vec<&Branch> = self.refs.local.iter().filter(matches).collect();
                if widgets::section(ui, "local", "laptop", "LOCAL", local.len(), true) {
                    for b in local {
                        let (ahead, behind) = b.ahead_behind();
                        // Checked out in another worktree: git won't check it out here too.
                        let elsewhere = self
                            .worktrees
                            .iter()
                            .find(|w| !w.current && w.branch.as_deref() == Some(b.name.as_str()));
                        let r = branch_row(
                            ui,
                            &b.name,
                            b.is_head,
                            ahead,
                            behind,
                            b.upstream_gone(),
                            elsewhere.is_some(),
                        );
                        let tip = match (b.upstream.is_empty(), b.upstream_gone()) {
                            (true, _) => "Not tracking a remote branch".to_owned(),
                            (false, true) => format!("{} was deleted on the remote", b.upstream),
                            (false, false) => format!("Tracks {}", b.upstream),
                        };
                        let r = match elsewhere {
                            Some(wt) => r.on_hover_text(format!(
                                "{tip}\nChecked out in worktree {}\nDouble-click to open it",
                                wt.path.display()
                            )),
                            None => r.on_hover_text(format!("{tip}\nDouble-click to check out")),
                        };
                        if r.double_clicked() && elsewhere.is_some() {
                            if let Some(wt) = elsewhere {
                                acts.push(Action::OpenTab(wt.path.clone()));
                            }
                        } else if r.double_clicked() && !b.is_head && idle {
                            acts.push(Action::Checkout(b.name.clone()));
                        } else if r.clicked() {
                            acts.push(Action::SelectHash(b.target.clone()));
                        }
                        r.context_menu(|ui| {
                            if let Some(wt) = elsewhere {
                                if ui.button("Open its worktree in a tab").clicked() {
                                    acts.push(Action::OpenTab(wt.path.clone()));
                                }
                            } else {
                                if ui.add_enabled(!b.is_head && idle, egui::Button::new("Check out")).clicked() {
                                    acts.push(Action::Checkout(b.name.clone()));
                                }
                                if ui
                                    .add_enabled(!b.is_head && idle, egui::Button::new("Check out in a new worktree…"))
                                    .clicked()
                                {
                                    acts.push(Action::Dialog(Dialog::add_worktree(&self.path, Some(&b.name), &self.refs.head)));
                                }
                            }
                            merge_item(ui, acts, &b.name, current.as_deref(), b.is_head || !idle);
                            if ui.button("Create branch here…").clicked() {
                                acts.push(Action::Dialog(Dialog::create_branch(&b.target, &b.name)));
                            }
                            ui.separator();
                            if ui.button("Copy name").clicked() {
                                ui.ctx().copy_text(b.name.clone());
                            }
                            ui.separator();
                            if ui
                                .add_enabled(!b.is_head && idle, egui::Button::new("Delete…"))
                                .on_disabled_hover_text("Can't delete the checked-out branch")
                                .clicked()
                            {
                                acts.push(Action::Dialog(Dialog::confirm(
                                    "Delete branch",
                                    format!("Delete the local branch “{}”?", b.name),
                                    "Delete",
                                    ops(&[&["branch", "-d", &b.name]]),
                                    Some((
                                        "Force, even if it isn't merged (-D)".to_owned(),
                                        ops(&[&["branch", "-D", &b.name]]),
                                    )),
                                )));
                            }
                        });
                    }
                }

                for (remote, branches) in &self.refs.remotes {
                    let list: Vec<&Branch> = branches.iter().filter(matches).collect();
                    let title = remote.to_uppercase();
                    if widgets::section(ui, &format!("remote:{remote}"), "cloud", &title, list.len(), true) {
                        for b in list {
                            let short = b
                                .name
                                .strip_prefix(&format!("{remote}/"))
                                .unwrap_or(&b.name);
                            let r = branch_row(ui, short, false, 0, 0, false, false)
                                .on_hover_text(format!("{}\nDouble-click to check out", b.name));
                            if r.double_clicked() && idle {
                                acts.push(Action::CheckoutRemote(b.name.clone()));
                            } else if r.clicked() {
                                acts.push(Action::SelectHash(b.target.clone()));
                            }
                            r.context_menu(|ui| {
                                if ui.add_enabled(idle, egui::Button::new("Check out as local branch")).clicked() {
                                    acts.push(Action::CheckoutRemote(b.name.clone()));
                                }
                                merge_item(ui, acts, &b.name, current.as_deref(), !idle);
                                if ui.button("Create branch here…").clicked() {
                                    acts.push(Action::Dialog(Dialog::create_branch(&b.target, &b.name)));
                                }
                                ui.separator();
                                if ui.button("Copy name").clicked() {
                                    ui.ctx().copy_text(b.name.clone());
                                }
                                ui.separator();
                                if ui.add_enabled(idle, egui::Button::new("Delete from remote…")).clicked() {
                                    acts.push(Action::Dialog(Dialog::confirm(
                                        "Delete remote branch",
                                        format!("Delete “{short}” from {remote}? This changes the remote for everyone."),
                                        "Delete",
                                        ops(&[&["push", remote, "--delete", short]]),
                                        None,
                                    )));
                                }
                            });
                        }
                    }
                }

                self.worktree_section(ui, acts, idle);

                let tags: Vec<&Branch> = self.refs.tags.iter().filter(matches).collect();
                if !tags.is_empty() && widgets::section(ui, "tags", "tag", "TAGS", tags.len(), false) {
                    for tag in tags {
                        let r = plain_row(ui, &tag.name);
                        if r.clicked() {
                            acts.push(Action::SelectHash(tag.target.clone()));
                        }
                        r.context_menu(|ui| {
                            if ui.button("Copy name").clicked() {
                                ui.ctx().copy_text(tag.name.clone());
                            }
                            if ui.add_enabled(idle, egui::Button::new("Delete tag…")).clicked() {
                                acts.push(Action::Dialog(Dialog::confirm(
                                    "Delete tag",
                                    format!("Delete the local tag “{}”?", tag.name),
                                    "Delete",
                                    ops(&[&["tag", "-d", &tag.name]]),
                                    None,
                                )));
                            }
                        });
                    }
                }
            });
    }
}

impl RepoTab {
    fn worktree_section(&self, ui: &mut Ui, acts: &mut Vec<Action>, idle: bool) {
        let t = Tokens::get(ui.ctx());
        let (open, add) = widgets::section_with_button(
            ui,
            "worktrees",
            "folder-tree",
            "WORKTREES",
            self.worktrees.len(),
            true,
            Some(("plus", "Add a worktree…")),
        );
        if add && idle {
            acts.push(Action::Dialog(Dialog::add_worktree(
                &self.path,
                None,
                &self.refs.head,
            )));
        }
        if !open {
            return;
        }
        let any_prunable = self.worktrees.iter().any(|w| w.prunable.is_some());
        for wt in &self.worktrees {
            let (rect, r) =
                ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
            let painter = ui.painter_at(rect);
            if wt.current {
                painter.rect_filled(rect, 0.0, t.accent.gamma_multiply(0.14));
            }
            if r.hovered() {
                painter.rect_filled(rect, 0.0, t.hover);
            }
            let y = rect.center().y;
            let icon = if wt.current { "check" } else { "folder-git-2" };
            icons::paint(
                ui,
                Rect::from_center_size(pos2(rect.left() + 34.0, y), vec2(16.0, 16.0)),
                icon,
                14.0,
                if wt.current { t.accent } else { t.text_faint },
            );
            let font = if wt.current {
                theme::semibold(12.5)
            } else {
                egui::FontId::proportional(12.5)
            };
            let mut right = rect.right() - 8.0;
            for (on, label, color) in [
                (wt.prunable.is_some(), "missing", t.danger),
                (wt.locked.is_some(), "locked", t.warning),
                (wt.main, "main", t.text_faint),
            ] {
                if on {
                    let r = painter.text(
                        pos2(right, y),
                        Align2::RIGHT_CENTER,
                        label,
                        theme::medium(10.5),
                        color,
                    );
                    right = r.left() - 6.0;
                }
            }
            let clip =
                painter.with_clip_rect(Rect::from_min_max(rect.min, pos2(right, rect.bottom())));
            clip.text(
                pos2(rect.left() + 48.0, y - 7.0),
                Align2::LEFT_CENTER,
                wt.name(),
                font,
                t.text,
            );
            let sub = match (&wt.branch, wt.bare) {
                (_, true) => "bare".to_owned(),
                (Some(branch), _) => branch.clone(),
                (None, _) => format!("detached at {}", wt.head.get(..8).unwrap_or(&wt.head)),
            };
            clip.text(
                pos2(rect.left() + 48.0, y + 8.0),
                Align2::LEFT_CENTER,
                sub,
                egui::FontId::proportional(11.0),
                t.text_faint,
            );

            let mut tip = wt.path.display().to_string();
            if let Some(reason) = wt.locked.as_deref().filter(|r| !r.is_empty()) {
                tip.push_str(&format!("\nLocked: {reason}"));
            }
            if let Some(reason) = &wt.prunable {
                tip.push_str(&format!("\nMissing: {reason}"));
            }
            if !wt.current {
                tip.push_str("\nDouble-click to open in a tab");
            }
            let r = r.on_hover_text(tip);
            if r.double_clicked() && !wt.current && wt.prunable.is_none() {
                acts.push(Action::OpenTab(wt.path.clone()));
            } else if r.clicked() && !wt.head.is_empty() {
                acts.push(Action::SelectHash(wt.head.clone()));
            }
            let path = wt.path.display().to_string();
            r.context_menu(|ui| {
                if ui
                    .add_enabled(!wt.current && wt.prunable.is_none(), egui::Button::new("Open in a tab"))
                    .clicked()
                {
                    acts.push(Action::OpenTab(wt.path.clone()));
                }
                if ui.button("Copy path").clicked() {
                    ui.ctx().copy_text(path.clone());
                }
                if !wt.main {
                    ui.separator();
                    let (label, args) = if wt.locked.is_some() {
                        ("Unlock", vec!["worktree", "unlock", path.as_str()])
                    } else {
                        ("Lock", vec!["worktree", "lock", path.as_str()])
                    };
                    if ui.add_enabled(idle, egui::Button::new(label)).clicked() {
                        acts.push(Action::Ops {
                            label: format!("{label} worktree {}", wt.name()),
                            ops: ops(&[&args]),
                            refresh: Refresh::All,
                        });
                    }
                    if ui
                        .add_enabled(idle && !wt.current, egui::Button::new("Remove…"))
                        .on_disabled_hover_text("Open another worktree to remove this one")
                        .clicked()
                    {
                        acts.push(Action::Dialog(Dialog::confirm(
                            "Remove worktree",
                            format!(
                                "Remove the worktree at {path}? Its folder is deleted; the branch stays."
                            ),
                            "Remove",
                            ops(&[&["worktree", "remove", &path]]),
                            Some((
                                "Force, even with uncommitted changes or locked".to_owned(),
                                ops(&[&["worktree", "remove", "--force", "--force", &path]]),
                            )),
                        )));
                    }
                }
                if any_prunable {
                    ui.separator();
                    if ui.add_enabled(idle, egui::Button::new("Prune missing worktrees")).clicked() {
                        acts.push(Action::Ops {
                            label: "Prune worktrees".into(),
                            ops: ops(&[&["worktree", "prune"]]),
                            refresh: Refresh::All,
                        });
                    }
                }
            });
        }
    }
}

fn merge_item(
    ui: &mut Ui,
    acts: &mut Vec<Action>,
    branch: &str,
    current: Option<&str>,
    disabled: bool,
) {
    let Some(current) = current else { return };
    if ui
        .add_enabled(
            !disabled,
            egui::Button::new(format!("Merge into {current}")),
        )
        .clicked()
    {
        acts.push(Action::Ops {
            label: format!("Merge {branch} into {current}"),
            ops: ops(&[&["merge", "--no-edit", branch]]),
            refresh: Refresh::All,
        });
    }
}

/// A branch: icon (a check for the checked-out one), name, ahead/behind counts.
fn branch_row(
    ui: &mut Ui,
    name: &str,
    is_head: bool,
    ahead: u32,
    behind: u32,
    gone: bool,
    in_worktree: bool,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let painter = ui.painter_at(rect);
    if is_head {
        painter.rect_filled(rect, 0.0, t.accent.gamma_multiply(0.14));
    }
    if resp.hovered() {
        painter.rect_filled(rect, 0.0, t.hover);
    }
    let y = rect.center().y;
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 34.0, y), vec2(16.0, 16.0));
    if is_head {
        icons::paint(ui, icon_rect, "check", 14.0, t.accent);
    } else {
        icons::paint(ui, icon_rect, "git-branch", 13.0, t.text_faint);
    }

    let mut right = rect.right() - 8.0;
    let small = theme::mono(11.0);
    if in_worktree {
        let ir = Rect::from_center_size(pos2(right - 6.0, y), vec2(12.0, 12.0));
        icons::paint(ui, ir, "folder-tree", 12.0, t.text_dim);
        right = ir.left() - 6.0;
    }
    if gone {
        let r = painter.text(
            pos2(right, y),
            Align2::RIGHT_CENTER,
            "gone",
            theme::medium(10.5),
            t.text_faint,
        );
        right = r.left() - 6.0;
    }
    for (n, icon) in [(behind, "arrow-down"), (ahead, "arrow-up")] {
        if n > 0 {
            let r = painter.text(
                pos2(right, y),
                Align2::RIGHT_CENTER,
                n.to_string(),
                small.clone(),
                t.text_dim,
            );
            let ir = Rect::from_center_size(pos2(r.left() - 7.0, y), vec2(12.0, 12.0));
            icons::paint(ui, ir, icon, 11.0, t.text_dim);
            right = ir.left() - 6.0;
        }
    }
    let font = if is_head {
        theme::semibold(12.5)
    } else {
        egui::FontId::proportional(12.5)
    };
    painter
        .with_clip_rect(Rect::from_min_max(
            pos2(rect.left(), rect.top()),
            pos2(right, rect.bottom()),
        ))
        .text(
            pos2(rect.left() + 48.0, y),
            Align2::LEFT_CENTER,
            name,
            font,
            t.text,
        );
    resp
}

fn plain_row(ui: &mut Ui, name: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, t.hover);
    }
    let y = rect.center().y;
    icons::paint(
        ui,
        Rect::from_center_size(pos2(rect.left() + 34.0, y), vec2(16.0, 16.0)),
        "tag",
        13.0,
        t.text_faint,
    );
    ui.painter_at(rect).text(
        pos2(rect.left() + 48.0, y),
        Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(12.5),
        t.text,
    );
    resp
}
