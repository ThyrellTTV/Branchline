//! Modal dialogs: create a branch, confirm a destructive operation.

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

    pub fn refresh(mut self, what: Refresh) -> Self {
        if let Dialog::Confirm { refresh, .. } = &mut self {
            *refresh = what;
        }
        self
    }
}

enum Outcome {
    Open,
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
        }
    }
}
