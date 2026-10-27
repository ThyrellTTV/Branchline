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
                        let r = branch_row(ui, &b.name, b.is_head, ahead, behind, b.upstream_gone());
                        let tip = match (b.upstream.is_empty(), b.upstream_gone()) {
                            (true, _) => "Not tracking a remote branch".to_owned(),
                            (false, true) => format!("{} was deleted on the remote", b.upstream),
                            (false, false) => format!("Tracks {}", b.upstream),
                        };
                        let r = r.on_hover_text(format!("{tip}\nDouble-click to check out"));
                        if r.double_clicked() && !b.is_head && idle {
                            acts.push(Action::Checkout(b.name.clone()));
                        } else if r.clicked() {
                            acts.push(Action::SelectHash(b.target.clone()));
                        }
                        r.context_menu(|ui| {
                            if ui.add_enabled(!b.is_head && idle, egui::Button::new("Check out")).clicked() {
                                acts.push(Action::Checkout(b.name.clone()));
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
                            let r = branch_row(ui, short, false, 0, 0, false)
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
