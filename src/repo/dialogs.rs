//! Modal dialogs: create a branch, confirm a destructive operation.

use std::path::{Path, PathBuf};

use egui::{Key, RichText};

use super::widgets;
use super::{Action, Refresh, RepoTab};
use crate::theme::{self, Tokens};

pub enum Dialog {
    CreateBranch {
        /// Commit the branch starts at.
        at: String,
        /// How to show `at` (branch name or short hash).
        at_label: String,
        name: String,
        checkout: bool,
        focus: bool,
    },
    AddWorktree {
        /// The repository, to suggest a folder next to it.
        repo: PathBuf,
        path: String,
        /// The user typed a path; stop suggesting one.
        path_edited: bool,
        /// Create a new branch instead of checking out an existing one.
        new_branch: bool,
        branch: String,
        new_name: String,
        /// Where a new branch starts (HEAD's commit).
        base: String,
        focus: bool,
    },
    Confirm {
        title: String,
        body: String,
        button: String,
        ops: Vec<Vec<String>>,
        /// An optional checkbox (label, ops to run instead when ticked), e.g. force delete.
        alternative: Option<(String, Vec<Vec<String>>)>,
        use_alternative: bool,
        refresh: Refresh,
    },
}

impl Dialog {
    pub fn create_branch(at: &str, at_label: &str) -> Self {
        Dialog::CreateBranch {
            at: at.to_owned(),
            at_label: at_label.to_owned(),
            name: String::new(),
            checkout: true,
            focus: true,
        }
    }

    pub fn confirm(
        title: &str,
        body: String,
        button: &str,
        ops: Vec<Vec<String>>,
        alternative: Option<(String, Vec<Vec<String>>)>,
    ) -> Self {
        Dialog::Confirm {
            title: title.to_owned(),
            body,
            button: button.to_owned(),
            ops,
            alternative,
            use_alternative: false,
            refresh: Refresh::All,
        }
    }

    /// Add a worktree, checking out `branch` (or a new branch from `base` when `None`).
    pub fn add_worktree(repo: &Path, branch: Option<&str>, base: &str) -> Self {
        let mut dialog = Dialog::AddWorktree {
            repo: repo.to_path_buf(),
            path: String::new(),
            path_edited: false,
            new_branch: branch.is_none(),
            branch: branch.unwrap_or_default().to_owned(),
            new_name: String::new(),
            base: base.to_owned(),
            focus: true,
        };
        dialog.suggest_path();
        dialog
    }

    /// A folder beside the repository named after the branch: `app` + `feature/x` → `app-feature-x`.
    fn suggest_path(&mut self) {
        if let Dialog::AddWorktree {
            repo,
            path,
            path_edited: false,
            new_branch,
            branch,
            new_name,
            ..
        } = self
        {
            let name = if *new_branch {
                new_name.as_str()
            } else {
                branch.as_str()
            };
            let repo_name = repo
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "worktree".to_owned());
            let folder = if name.trim().is_empty() {
                format!("{repo_name}-worktree")
            } else {
                format!("{repo_name}-{}", name.trim().replace(['/', '\\', ' '], "-"))
            };
            let parent = repo.parent().unwrap_or(repo.as_path());
            *path = parent.join(folder).display().to_string();
        }
    }

    pub fn refresh(mut self, what: Refresh) -> Self {
        if let Dialog::Confirm { refresh, .. } = &mut self {
            *refresh = what;
        }
        self
    }
}

enum Outcome {
    Open,
    /// Still open; the branch changed, so suggest a folder again.
    Suggest,
    Cancel,
    Done(Action),
}

impl RepoTab {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context, acts: &mut Vec<Action>) {
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let t = Tokens::get(ctx);
        let frame = egui::Frame::new()
            .fill(t.card)
            .stroke(egui::Stroke::new(1.0, t.field_border))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(18))
            .shadow(ctx.global_style().visuals.window_shadow);
        let modal = egui::Modal::new(egui::Id::new("gitcraft-dialog"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(110));

        let local = &self.refs.local;
        let worktrees = &self.worktrees;
        let response = modal.show(ctx, |ui| {
            ui.set_width(380.0);
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            match dialog {
                Dialog::CreateBranch {
                    at,
                    at_label,
                    name,
                    checkout,
                    focus,
                } => {
                    ui.label(RichText::new("Create branch").font(theme::semibold(15.0)));
                    ui.label(RichText::new(format!("Starting at {at_label}")).color(t.text_dim));
                    ui.add_space(10.0);
                    let edit = ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text("feature/my-branch")
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(8, 6)),
                    );
                    if *focus {
                        edit.request_focus();
                        *focus = false;
                    }
                    // Spaces are never valid in a branch name; typing one gives a dash.
                    if name.contains(' ') {
                        *name = name.replace(' ', "-");
                    }
                    ui.add_space(6.0);
                    ui.checkbox(checkout, "Check it out");
                    ui.add_space(12.0);
                    let valid = !name.trim().is_empty();
                    let mut outcome = Outcome::Open;
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, "Create", valid, 90.0).clicked()
                            || (enter && valid)
                        {
                            let name = name.trim().to_owned();
                            let args: Vec<String> = if *checkout {
                                vec!["switch".into(), "-c".into(), name.clone(), at.clone()]
                            } else {
                                vec!["branch".into(), name.clone(), at.clone()]
                            };
                            outcome = Outcome::Done(Action::Ops {
                                label: format!("Create branch {name}"),
                                ops: vec![args],
                                refresh: Refresh::All,
                            });
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 30.0)))
                            .clicked()
                        {
                            outcome = Outcome::Cancel;
                        }
                    });
                    outcome
                }
                Dialog::AddWorktree {
                    path,
                    path_edited,
                    new_branch,
                    branch,
                    new_name,
                    base,
                    focus,
                    ..
                } => {
                    ui.label(RichText::new("Add worktree").font(theme::semibold(15.0)));
                    ui.label(
                        RichText::new(
                            "A second working folder of this repository, on its own branch.",
                        )
                        .color(t.text_dim),
                    );
                    ui.add_space(10.0);
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        changed |= ui
                            .radio_value(new_branch, false, "Existing branch")
                            .changed();
                        changed |= ui.radio_value(new_branch, true, "New branch").changed();
                    });
                    ui.add_space(4.0);
                    if *new_branch {
                        let edit = ui.add(
                            egui::TextEdit::singleline(new_name)
                                .hint_text("feature/my-branch")
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if *focus {
                            edit.request_focus();
                            *focus = false;
                        }
                        if new_name.contains(' ') {
                            *new_name = new_name.replace(' ', "-");
                        }
                        changed |= edit.changed();
                        ui.label(
                            RichText::new(format!(
                                "Starting at HEAD ({})",
                                base.get(..8).unwrap_or(base)
                            ))
                            .color(t.text_faint)
                            .small(),
                        );
                    } else {
                        // Branches that aren't checked out in any worktree yet.
                        let busy: Vec<&str> = worktrees
                            .iter()
                            .filter_map(|w| w.branch.as_deref())
                            .collect();
                        let free: Vec<&str> = local
                            .iter()
                            .map(|b| b.name.as_str())
                            .filter(|n| !busy.contains(n))
                            .collect();
                        egui::ComboBox::from_id_salt("worktree-branch")
                            .width(ui.available_width())
                            .selected_text(if branch.is_empty() {
                                "Choose a branch"
                            } else {
                                branch.as_str()
                            })
                            .show_ui(ui, |ui| {
                                for name in &free {
                                    if ui.selectable_label(branch == name, *name).clicked() {
                                        *branch = (*name).to_owned();
                                        changed = true;
                                    }
                                }
                                if free.is_empty() {
                                    ui.label(
                                        RichText::new("Every branch is checked out somewhere")
                                            .color(t.text_faint),
                                    );
                                }
                            });
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("Folder").color(t.text_dim).small());
                    ui.horizontal(|ui| {
                        let edit = ui.add(
                            egui::TextEdit::singleline(path)
                                .desired_width(ui.available_width() - 90.0)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if edit.changed() {
                            *path_edited = true;
                        }
                        if ui
                            .add(egui::Button::new("Browse…").min_size(egui::vec2(80.0, 28.0)))
                            .clicked()
                            && let Some(dir) = rfd::FileDialog::new()
                                .set_title("Worktree folder")
                                .pick_folder()
                        {
                            *path = dir.display().to_string();
                            *path_edited = true;
                        }
                    });
                    ui.add_space(12.0);
                    let name_ok = if *new_branch {
                        !new_name.trim().is_empty()
                    } else {
                        !branch.is_empty()
                    };
                    let valid = name_ok && !path.trim().is_empty();
                    let mut outcome = Outcome::Open;
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, "Add", valid, 90.0).clicked()
                            || (enter && valid)
                        {
                            let path = path.trim().to_owned();
                            let args: Vec<String> = if *new_branch {
                                vec![
                                    "worktree".into(),
                                    "add".into(),
                                    "-b".into(),
                                    new_name.trim().to_owned(),
                                    path.clone(),
                                    base.clone(),
                                ]
                            } else {
                                vec![
                                    "worktree".into(),
                                    "add".into(),
                                    path.clone(),
                                    branch.clone(),
                                ]
                            };
                            outcome = Outcome::Done(Action::Ops {
                                label: "Add worktree".to_owned(),
                                ops: vec![args],
                                refresh: Refresh::All,
                            });
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 30.0)))
                            .clicked()
                        {
                            outcome = Outcome::Cancel;
                        }
                    });
                    if changed {
                        outcome = match outcome {
                            Outcome::Open => Outcome::Suggest,
                            other => other,
                        };
                    }
                    outcome
                }
                Dialog::Confirm {
                    title,
                    body,
                    button,
                    ops,
                    alternative,
                    use_alternative,
                    refresh,
                } => {
                    ui.label(RichText::new(title.as_str()).font(theme::semibold(15.0)));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(body.as_str()).wrap());
                    if let Some((label, _)) = alternative {
                        ui.add_space(6.0);
                        ui.checkbox(use_alternative, label.as_str());
                    }
                    ui.add_space(14.0);
                    let mut outcome = Outcome::Open;
                    ui.horizontal(|ui| {
                        let danger = egui::Button::new(
                            RichText::new(button.as_str())
                                .color(egui::Color32::WHITE)
                                .font(theme::medium(12.5)),
                        )
                        .fill(t.danger)
                        .min_size(egui::vec2(90.0, 30.0));
                        if ui.add(danger).clicked() {
                            let steps = match (alternative, *use_alternative) {
                                (Some((_, alt)), true) => alt.clone(),
                                _ => ops.clone(),
                            };
                            outcome = Outcome::Done(Action::Ops {
                                label: title.clone(),
                                ops: steps,
                                refresh: *refresh,
                            });
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 30.0)))
                            .clicked()
                        {
                            outcome = Outcome::Cancel;
                        }
                    });
                    outcome
                }
            }
        });

        match response.inner {
            Outcome::Done(action) => {
                acts.push(action);
                self.dialog = None;
            }
            Outcome::Cancel => self.dialog = None,
            Outcome::Open if response.should_close() => self.dialog = None,
            Outcome::Open => {}
            Outcome::Suggest => dialog.suggest_path(),
        }
    }
}
