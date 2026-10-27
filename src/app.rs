//! Window shell: git check, browser-like tabs, the start page and cloning.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::{Align2, CornerRadius, Key, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::git;
use crate::icons;
use crate::repo::RepoTab;
use crate::theme::{self, ThemeKind, Tokens};

const MAX_RECENT: usize = 12;
const TAB_H: f32 = 34.0;

/// What survives a restart.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    open: Vec<PathBuf>,
    active: usize,
    recent: Vec<PathBuf>,
    theme: ThemeKind,
    clone_parent: Option<PathBuf>,
}

enum Tab {
    /// The start page ("new tab"): open, create or clone a repository.
    Home,
    Repo(Box<RepoTab>),
}

struct CloneForm {
    url: String,
    parent: String,
    name: String,
    /// The user typed a name; stop deriving it from the URL.
    name_edited: bool,
    error: Option<String>,
    focus: bool,
}

pub struct App {
    git_version: Result<String, String>,
    tabs: Vec<Tab>,
    active: usize,
    recent: Vec<PathBuf>,
    theme: ThemeKind,
    clone_parent: Option<PathBuf>,
    error: Option<String>,
    was_focused: bool,
    clone_form: Option<CloneForm>,
    /// A clone running in the background: (destination, result).
    cloning: Option<(PathBuf, Receiver<Result<(), String>>)>,
}

impl App {
    /// `open_arg`: a repository passed on the command line, opened (or focused) on start.
    pub fn new(cc: &eframe::CreationContext<'_>, open_arg: Option<PathBuf>) -> Self {
        let saved: Saved = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
        theme::install_fonts(ctx);
        theme::apply(ctx, saved.theme);

        let mut app = Self {
            git_version: git::version(),
            tabs: Vec::new(),
            active: 0,
            recent: saved.recent,
            theme: saved.theme,
            clone_parent: saved.clone_parent,
            error: None,
            was_focused: true,
            clone_form: None,
            cloning: None,
        };
        if app.git_version.is_ok() {
            for path in saved.open {
                if path.is_dir() {
                    app.tabs.push(Tab::Repo(Box::new(RepoTab::new(path, ctx))));
                }
            }
        }
        if app.tabs.is_empty() {
            app.tabs.push(Tab::Home);
        }
        app.active = saved.active.min(app.tabs.len() - 1);
        if let Some(path) = open_arg
            && app.git_version.is_ok()
        {
            app.open_repo(&path, ctx);
        }
        app
    }

    /// Opens `path` in the active tab if it's a start page, otherwise in a new tab.
    fn open_repo(&mut self, path: &Path, ctx: &egui::Context) {
        let root = match git::toplevel(path) {
            Ok(root) => root,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        let root = strip_verbatim(root);
        self.remember(&root);
        self.error = None;

        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| matches!(t, Tab::Repo(r) if r.path == root))
        {
            self.active = i;
            return;
        }
        let tab = Tab::Repo(Box::new(RepoTab::new(root, ctx)));
        if matches!(self.tabs.get(self.active), Some(Tab::Home)) {
            self.tabs[self.active] = tab;
        } else {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
    }

    fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(MAX_RECENT);
    }

    fn pick_existing(&mut self, ctx: &egui::Context) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Open a git repository")
            .pick_folder()
        {
            self.open_repo(&dir, ctx);
        }
    }

    fn pick_new(&mut self, ctx: &egui::Context) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("Choose a folder for the new repository")
            .pick_folder()
        else {
            return;
        };
        // Already the root of a repository: just open it rather than re-initialising.
        if let Ok(root) = git::toplevel(&dir)
            && same_path(&root, &dir)
        {
            self.open_repo(&dir, ctx);
            return;
        }
        match git::init(&dir) {
            Ok(()) => self.open_repo(&dir, ctx),
            Err(e) => self.error = Some(format!("git init failed: {e}")),
        }
    }

    fn start_clone(&mut self) {
        let parent = self
            .clone_parent
            .clone()
            .or_else(|| {
                self.recent
                    .first()
                    .and_then(|p| p.parent())
                    .map(Path::to_path_buf)
            })
            .or_else(home_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.clone_form = Some(CloneForm {
            url: String::new(),
            parent,
            name: String::new(),
            name_edited: false,
            error: None,
            focus: true,
        });
    }

    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::Home);
        }
        if self.active > i || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
    }

    fn new_tab(&mut self) {
        self.tabs.push(Tab::Home);
        self.active = self.tabs.len() - 1;
    }

    fn set_theme(&mut self, ctx: &egui::Context, kind: ThemeKind) {
        self.theme = kind;
        theme::apply(ctx, kind);
    }

    /// PhotoCraft-style document tabs on a dark strip, `+` after them, the theme menu at the end.
    fn tab_strip(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let mut select = None;
        let mut close = None;
        let mut add = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            for (i, tab) in self.tabs.iter().enumerate() {
                let active = i == self.active;
                let (title, tooltip, busy) = match tab {
                    Tab::Home => (
                        "New tab".to_owned(),
                        "Open, create or clone a repository".to_owned(),
                        false,
                    ),
                    Tab::Repo(r) => (r.name.clone(), r.path.display().to_string(), r.is_busy()),
                };
                let font = if active {
                    theme::medium(12.5)
                } else {
                    egui::FontId::proportional(12.5)
                };
                let color = if active { t.text } else { t.text_dim };
                let galley = ui.painter().layout_no_wrap(title, font, color);
                let w = (galley.size().x + 66.0).clamp(110.0, 260.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(w, TAB_H), Sense::click());
                let hovered = resp.hovered();
                let p = ui.painter();
                let shape = CornerRadius {
                    nw: t.radius as u8,
                    ne: t.radius as u8,
                    sw: 0,
                    se: 0,
                };
                if active {
                    p.rect_filled(rect, shape, t.chrome);
                    p.line_segment(
                        [
                            rect.left_top() + vec2(2.0, 0.5),
                            rect.right_top() + vec2(-2.0, 0.5),
                        ],
                        Stroke::new(2.0, t.accent),
                    );
                } else if hovered {
                    p.rect_filled(rect, shape, t.hover.gamma_multiply(0.6));
                }
                let y = rect.center().y;
                let icon = if matches!(tab, Tab::Home) {
                    "plus"
                } else {
                    "folder-git-2"
                };
                icons::paint(
                    ui,
                    Rect::from_center_size(pos2(rect.left() + 18.0, y), vec2(16.0, 16.0)),
                    icon,
                    14.0,
                    if active { t.accent } else { t.text_faint },
                );
                let text_clip = Rect::from_min_max(
                    pos2(rect.left() + 32.0, rect.top()),
                    pos2(rect.right() - 28.0, rect.bottom()),
                );
                ui.painter().with_clip_rect(text_clip).galley(
                    pos2(text_clip.left(), y - galley.size().y / 2.0),
                    galley,
                    color,
                );

                let close_rect =
                    Rect::from_center_size(pos2(rect.right() - 15.0, y), vec2(18.0, 18.0));
                if busy {
                    egui::Spinner::new().size(11.0).paint_at(ui, close_rect);
                } else if active || hovered {
                    let c = ui.interact(close_rect, resp.id.with("close"), Sense::click());
                    if c.hovered() {
                        ui.painter().rect_filled(close_rect, t.radius_sm, t.pressed);
                    }
                    icons::paint(
                        ui,
                        close_rect,
                        "x",
                        12.0,
                        if c.hovered() { t.text } else { t.text_faint },
                    );
                    if c.clicked() {
                        close = Some(i);
                    }
                    c.on_hover_text("Close tab (Ctrl+W)");
                }
                let resp = resp.on_hover_text(tooltip);
                if resp.clicked() && close.is_none() {
                    select = Some(i);
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
            }
            ui.add_space(4.0);
            if icons::small_button(ui, "plus", "New tab (Ctrl+T)").clicked() {
                add = true;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                let resp = icons::small_button(ui, "palette", "Theme");
                egui::Popup::menu(&resp).show(|ui| {
                    ui.label(
                        RichText::new("THEME")
                            .font(theme::semibold(10.5))
                            .color(t.text_faint),
                    );
                    for kind in ThemeKind::ALL {
                        if ui.radio(self.theme == kind, kind.label()).clicked() {
                            self.set_theme(ui.ctx(), kind);
                        }
                    }
                });
            });
        });
        if let Some(i) = select {
            self.active = i;
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
        if add {
            self.new_tab();
        }
    }

    fn home(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let t = Tokens::get(&ctx);
        let mut open = None;
        let mut forget = None;
        let mut action = None;

        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(640.0);
                    ui.add_space((ui.available_height() * 0.10).max(32.0));
                    ui.horizontal(|ui| {
                        icons::inline(ui, "git-merge", 34.0, t.accent);
                        ui.label(RichText::new("gitcraft").font(theme::semibold(30.0)));
                    });
                    ui.add_space(4.0);
                    if let Ok(v) = &self.git_version {
                        ui.horizontal(|ui| {
                            icons::inline(ui, "check", 14.0, t.success);
                            ui.label(RichText::new(v).color(t.text_dim));
                        });
                    }
                    ui.add_space(26.0);

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let w = (ui.available_width() - 24.0) / 3.0;
                        for (i, (icon, title, sub)) in [
                            ("folder-open", "Open", "An existing local repository"),
                            ("folder-plus", "Create", "A new repository (git init)"),
                            ("log-in", "Clone", "Copy a remote repository"),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            if action_card(ui, w, icon, title, sub).clicked() {
                                action = Some(i);
                            }
                        }
                    });

                    if let Some(err) = &self.error {
                        ui.add_space(12.0);
                        ui.colored_label(t.danger, err);
                    }
                    if let Some((dest, _)) = &self.cloning {
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("Cloning into {}…", dest.display()));
                        });
                    }

                    if !self.recent.is_empty() {
                        ui.add_space(30.0);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.label(
                                RichText::new("RECENT")
                                    .font(theme::semibold(10.5))
                                    .color(t.text_faint),
                            );
                            ui.add_space(4.0);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for (i, path) in self.recent.iter().enumerate() {
                                match recent_row(ui, path) {
                                    Some(RecentClick::Open) => open = Some(path.clone()),
                                    Some(RecentClick::Forget) => forget = Some(i),
                                    None => {}
                                }
                            }
                        });
                    }
                });
            });

        match action {
            Some(0) => self.pick_existing(&ctx),
            Some(1) => self.pick_new(&ctx),
            Some(_) => self.start_clone(),
            None => {}
        }
        if let Some(path) = open {
            self.open_repo(&path, &ctx);
        }
        if let Some(i) = forget {
            self.recent.remove(i);
        }
    }

    fn clone_dialog(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.clone_form else {
            return;
        };
        let t = Tokens::get(ctx);
        let frame = egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.field_border))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(18))
            .shadow(ctx.global_style().visuals.window_shadow);
        let mut submit = false;
        let mut cancel = false;
        let resp = egui::Modal::new(egui::Id::new("clone-dialog"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(110))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new("Clone repository").font(theme::semibold(15.0)));
                ui.add_space(10.0);
                ui.label(RichText::new("URL").color(t.text_dim).small());
                let url = ui.add(
                    egui::TextEdit::singleline(&mut form.url)
                        .hint_text("https://github.com/user/repo.git")
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 6)),
                );
                if form.focus {
                    url.request_focus();
                    form.focus = false;
                }
                if url.changed() && !form.name_edited {
                    form.name = git::clone_dir_name(&form.url);
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Into folder").color(t.text_dim).small());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut form.parent)
                            .desired_width(ui.available_width() - 90.0)
                            .margin(egui::Margin::symmetric(8, 6)),
                    );
                    if ui
                        .add(egui::Button::new("Browse…").min_size(vec2(80.0, 28.0)))
                        .clicked()
                        && let Some(dir) =
                            rfd::FileDialog::new().set_title("Clone into").pick_folder()
                    {
                        form.parent = dir.display().to_string();
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Name").color(t.text_dim).small());
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut form.name)
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(8, 6)),
                    )
                    .changed()
                {
                    form.name_edited = true;
                }
                let dest = PathBuf::from(form.parent.trim()).join(form.name.trim());
                ui.label(
                    RichText::new(dest.display().to_string())
                        .color(t.text_faint)
                        .small(),
                );
                if let Some(err) = &form.error {
                    ui.colored_label(t.danger, err);
                }
                ui.add_space(12.0);
                let valid = !form.url.trim().is_empty()
                    && !form.parent.trim().is_empty()
                    && !form.name.trim().is_empty();
                ui.horizontal(|ui| {
                    let enter = ui.input(|i| i.key_pressed(Key::Enter));
                    if crate::repo::primary_button(ui, "Clone", valid, 90.0).clicked()
                        || (enter && valid)
                    {
                        submit = true;
                    }
                    if ui
                        .add(egui::Button::new("Cancel").min_size(vec2(80.0, 30.0)))
                        .clicked()
                    {
                        cancel = true;
                    }
                });
            });
        if resp.should_close() {
            cancel = true;
        }

        if submit {
            let dest = PathBuf::from(form.parent.trim()).join(form.name.trim());
            let occupied = dest.exists()
                && std::fs::read_dir(&dest)
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(true);
            if occupied {
                form.error = Some(format!("{} already exists and isn't empty", dest.display()));
                return;
            }
            let url = form.url.trim().to_owned();
            self.clone_parent = Some(PathBuf::from(form.parent.trim()));
            let (tx, rx) = channel();
            let ctx2 = ctx.clone();
            let dest2 = dest.clone();
            std::thread::spawn(move || {
                let _ = tx.send(git::clone(&url, &dest2));
                ctx2.request_repaint();
            });
            self.cloning = Some((dest, rx));
            self.clone_form = None;
            self.error = None;
        } else if cancel {
            self.clone_form = None;
        }
    }

    fn poll_clone(&mut self, ctx: &egui::Context) {
        let Some((dest, rx)) = &self.cloning else {
            return;
        };
        let Ok(result) = rx.try_recv() else { return };
        let dest = dest.clone();
        self.cloning = None;
        match result {
            Ok(()) => {
                // Show it in a start page tab if one is active, otherwise in a new tab.
                self.open_repo(&dest, ctx);
            }
            Err(e) => self.error = Some(format!("Clone failed: {e}")),
        }
    }

    fn git_missing(&mut self, ui: &mut Ui, err: &str) {
        let t = Tokens::get(ui.ctx());
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            icons::inline(ui, "circle-alert", 40.0, t.warning);
            ui.add_space(8.0);
            ui.label(RichText::new("Git was not found").font(theme::semibold(24.0)));
            ui.add_space(8.0);
            ui.label("gitcraft uses the git command line. Install git and make sure `git` is on your PATH.");
            ui.add_space(4.0);
            ui.label(RichText::new(err).color(t.text_faint).small());
            ui.add_space(16.0);
            if crate::repo::primary_button(ui, "Check again", true, 140.0).clicked() {
                self.git_version = git::version();
            }
            ui.add_space(8.0);
            ui.hyperlink_to("Download git", "https://git-scm.com/downloads");
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t = Tokens::get(&ctx);

        if let Err(err) = self.git_version.clone() {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(t.dock))
                .show(ui, |ui| self.git_missing(ui, &err));
            return;
        }
        self.poll_clone(&ctx);

        // Shortcuts.
        // A focused text field or the terminal keeps these keys for itself.
        let modal_open = self.clone_form.is_some() || ctx.egui_wants_keyboard_input();
        let (new_tab, close, refresh, focused) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(Key::T),
                i.modifiers.command && i.key_pressed(Key::W),
                i.key_pressed(Key::F5),
                i.focused,
            )
        });
        if new_tab && !modal_open {
            self.new_tab();
        }
        if close && !modal_open {
            self.close_tab(self.active);
        }
        // Coming back to the window (e.g. after using git elsewhere) refreshes the open repo.
        let regained = focused && !self.was_focused;
        self.was_focused = focused;
        if let Some(Tab::Repo(r)) = self.tabs.get_mut(self.active)
            && ((refresh && !modal_open) || regained)
        {
            r.reload(&ctx);
        }

        // Background repos still need their results drained.
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if let Tab::Repo(r) = tab
                && i != self.active
            {
                r.poll(&ctx);
            }
        }

        egui::Panel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(t.tab_strip)
                    .inner_margin(egui::Margin {
                        left: 6,
                        right: 6,
                        top: 6,
                        bottom: 0,
                    }),
            )
            .show(ui, |ui| self.tab_strip(ui));

        let active = self.active;
        match self.tabs.get_mut(active) {
            Some(Tab::Repo(repo)) => {
                repo.ui(ui);
                // A worktree asked to be opened in its own tab.
                if let Some(path) = repo.open_request.take() {
                    self.open_repo(&path, &ctx);
                }
            }
            _ => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(t.dock))
                    .show(ui, |ui| self.home(ui));
            }
        }
        self.clone_dialog(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let open: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter_map(|t| match t {
                Tab::Repo(r) => Some(r.path.clone()),
                Tab::Home => None,
            })
            .collect();
        // Index of the active tab among the saved (repo) tabs.
        let active = self
            .tabs
            .iter()
            .take(self.active + 1)
            .filter(|t| matches!(t, Tab::Repo(_)))
            .count()
            .saturating_sub(1);
        let saved = Saved {
            open,
            active,
            recent: self.recent.clone(),
            theme: self.theme,
            clone_parent: self.clone_parent.clone(),
        };
        eframe::set_value(storage, eframe::APP_KEY, &saved);
    }
}

/// A large clickable card on the start page.
fn action_card(ui: &mut Ui, width: f32, icon: &str, title: &str, subtitle: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 116.0), Sense::click());
    let fill = if resp.hovered() { t.hover } else { t.card };
    let stroke = if resp.hovered() {
        t.accent
    } else {
        t.field_border
    };
    let p = ui.painter();
    p.rect(
        rect,
        t.radius_lg,
        fill,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    icons::paint(
        ui,
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 34.0), vec2(28.0, 28.0)),
        icon,
        26.0,
        t.accent,
    );
    p.text(
        pos2(rect.center().x, rect.top() + 68.0),
        Align2::CENTER_CENTER,
        title,
        theme::semibold(14.0),
        t.text,
    );
    p.text(
        pos2(rect.center().x, rect.top() + 90.0),
        Align2::CENTER_CENTER,
        subtitle,
        egui::FontId::proportional(11.5),
        t.text_dim,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

enum RecentClick {
    Open,
    Forget,
}

fn recent_row(ui: &mut Ui, path: &Path) -> Option<RecentClick> {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let y = rect.center().y;
    icons::paint(
        ui,
        Rect::from_center_size(pos2(rect.left() + 20.0, y), vec2(18.0, 18.0)),
        "folder-git-2",
        16.0,
        t.icon,
    );
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Leave room for the remove button; long paths are cut at the row's edge.
    let p = ui.painter_at(rect.with_max_x(rect.right() - 36.0));
    p.text(
        pos2(rect.left() + 40.0, y - 8.0),
        Align2::LEFT_CENTER,
        name,
        theme::medium(12.5),
        t.text,
    );
    p.text(
        pos2(rect.left() + 40.0, y + 8.0),
        Align2::LEFT_CENTER,
        path.display().to_string(),
        egui::FontId::proportional(11.0),
        t.text_faint,
    );
    let mut forget = false;
    if hovered || ui.rect_contains_pointer(rect) {
        let x_rect = Rect::from_center_size(pos2(rect.right() - 18.0, y), vec2(20.0, 20.0));
        let x = ui
            .interact(x_rect, resp.id.with("forget"), Sense::click())
            .on_hover_text("Remove from list");
        if x.hovered() {
            ui.painter().rect_filled(x_rect, t.radius_sm, t.pressed);
        }
        icons::paint(ui, x_rect, "x", 12.0, t.text_faint);
        forget = x.clicked();
    }
    if forget {
        Some(RecentClick::Forget)
    } else if resp.clicked() {
        Some(RecentClick::Open)
    } else {
        None
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `canonicalize` on Windows returns `\\?\C:\…`; show and store the familiar form.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path,
    }
}
