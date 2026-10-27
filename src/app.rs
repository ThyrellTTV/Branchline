//! Window shell: git check, browser-like tabs, the start page and cloning.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::{Align2, CornerRadius, Key, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::git;
use crate::icons;
use crate::repo::RepoTab;
use crate::settings::Settings;
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
    settings: Settings,
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
    settings: Settings,
    settings_open: bool,
    palette: Option<Palette>,
    toasts: Vec<Toast>,
}

/// The command palette (Ctrl+P): type to filter, Enter runs.
struct Palette {
    query: String,
    selected: usize,
    focus: bool,
}

struct Toast {
    text: String,
    error: bool,
    until: std::time::Instant,
}

/// What a palette entry does outside the open repository.
enum AppCommand {
    NewTab,
    Open,
    Create,
    Clone,
    Settings,
    Theme(ThemeKind),
}

enum Entry {
    App(AppCommand),
    /// Index into the active repository's palette items.
    Repo(usize),
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
        saved.settings.set(ctx);

        let mut app = Self {
            git_version: git::version(),
            tabs: Vec::new(),
            active: 0,
            recent: saved.recent,
            theme: saved.theme,
            clone_parent: saved.clone_parent,
            settings: saved.settings.clone(),
            settings_open: false,
            palette: None,
            toasts: Vec::new(),
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
                if icons::small_button(ui, "settings", "Settings").clicked() {
                    self.settings_open = true;
                }
                if icons::small_button(ui, "command", "Command palette (Ctrl+P)").clicked() {
                    self.open_palette();
                }
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
        let modal_open = self.clone_form.is_some()
            || self.palette.is_some()
            || self.settings_open
            || ctx.egui_wants_keyboard_input();
        let (new_tab, close, refresh, focused, palette) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(Key::T),
                i.modifiers.command && i.key_pressed(Key::W),
                i.key_pressed(Key::F5),
                i.focused,
                i.modifiers.command && i.key_pressed(Key::P),
            )
        });
        if palette && self.clone_form.is_none() && !self.settings_open {
            if self.palette.is_some() {
                self.palette = None;
            } else {
                self.open_palette();
            }
        }
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
                repo.keys_blocked =
                    self.palette.is_some() || self.settings_open || self.clone_form.is_some();
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
        self.palette_ui(&ctx);
        self.settings_ui(&ctx);

        // Long operations that ended: a toast, and a desktop notification when away.
        for tab in &mut self.tabs {
            if let Tab::Repo(r) = tab
                && let Some((text, error)) = r.finished.take()
            {
                if !focused && self.settings.notifications {
                    notify(&text);
                }
                self.toasts.push(Toast {
                    text,
                    error,
                    until: std::time::Instant::now() + std::time::Duration::from_secs(6),
                });
            }
        }
        self.toasts_ui(&ctx);
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
            settings: self.settings.clone(),
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

impl App {
    fn open_palette(&mut self) {
        self.palette = Some(Palette {
            query: String::new(),
            selected: 0,
            focus: true,
        });
    }

    /// Every palette entry: (label, hint, what it runs).
    fn palette_entries(&self) -> Vec<(String, String, Entry)> {
        let mut list = vec![
            (
                "New tab".to_owned(),
                "Ctrl+T".to_owned(),
                Entry::App(AppCommand::NewTab),
            ),
            (
                "Open repository…".to_owned(),
                "app".to_owned(),
                Entry::App(AppCommand::Open),
            ),
            (
                "Create repository…".to_owned(),
                "app".to_owned(),
                Entry::App(AppCommand::Create),
            ),
            (
                "Clone repository…".to_owned(),
                "app".to_owned(),
                Entry::App(AppCommand::Clone),
            ),
            (
                "Settings".to_owned(),
                "app".to_owned(),
                Entry::App(AppCommand::Settings),
            ),
        ];
        for kind in ThemeKind::ALL {
            list.push((
                format!("Theme: {}", kind.label()),
                "theme".to_owned(),
                Entry::App(AppCommand::Theme(kind)),
            ));
        }
        if let Some(Tab::Repo(repo)) = self.tabs.get(self.active) {
            for (i, (label, hint, _)) in repo.palette_items().into_iter().enumerate() {
                list.push((label, hint, Entry::Repo(i)));
            }
        }
        list
    }

    fn run_entry(&mut self, entry: Entry, ctx: &egui::Context) {
        match entry {
            Entry::App(AppCommand::NewTab) => self.new_tab(),
            Entry::App(AppCommand::Open) => self.pick_existing(ctx),
            Entry::App(AppCommand::Create) => self.pick_new(ctx),
            Entry::App(AppCommand::Clone) => self.start_clone(),
            Entry::App(AppCommand::Settings) => self.settings_open = true,
            Entry::App(AppCommand::Theme(kind)) => self.set_theme(ctx, kind),
            Entry::Repo(i) => {
                if let Some(Tab::Repo(repo)) = self.tabs.get_mut(self.active)
                    && let Some((_, _, action)) = repo.palette_items().into_iter().nth(i)
                {
                    repo.queue(action);
                    ctx.request_repaint();
                }
            }
        }
    }

    fn palette_ui(&mut self, ctx: &egui::Context) {
        if self.palette.is_none() {
            return;
        }
        let t = Tokens::get(ctx);
        let entries = self.palette_entries();
        let Some(palette) = &mut self.palette else {
            return;
        };
        let query = palette.query.to_lowercase();
        let shown: Vec<usize> = (0..entries.len())
            .filter(|&i| fuzzy(&entries[i].0.to_lowercase(), &query))
            .take(60)
            .collect();
        palette.selected = palette.selected.min(shown.len().saturating_sub(1));
        let (up, down, enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
            )
        });
        if down {
            palette.selected = (palette.selected + 1).min(shown.len().saturating_sub(1));
        }
        if up {
            palette.selected = palette.selected.saturating_sub(1);
        }
        let mut run = None;
        let mut close = esc;
        let frame = egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.field_border))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(10))
            .shadow(ctx.global_style().visuals.window_shadow);
        let area = egui::Area::new(egui::Id::new("palette"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, vec2(0.0, 70.0));
        let resp = area.show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(560.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut palette.query)
                        .hint_text("Type a command or a branch…")
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(10, 8))
                        .font(theme::medium(14.0)),
                );
                if std::mem::take(&mut palette.focus) {
                    edit.request_focus();
                }
                if edit.changed() {
                    palette.selected = 0;
                }
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (k, &i) in shown.iter().enumerate() {
                            let (label, hint, _) = &entries[i];
                            let (rect, r) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 28.0),
                                Sense::click(),
                            );
                            if k == palette.selected {
                                ui.painter().rect_filled(rect, t.radius_sm, t.row_selected);
                                if up || down {
                                    ui.scroll_to_rect(rect, None);
                                }
                            } else if r.hovered() {
                                ui.painter().rect_filled(rect, t.radius_sm, t.hover);
                            }
                            ui.painter().text(
                                pos2(rect.left() + 10.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                label,
                                egui::FontId::proportional(13.0),
                                t.text,
                            );
                            ui.painter().text(
                                pos2(rect.right() - 10.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                hint,
                                egui::FontId::proportional(11.0),
                                t.text_faint,
                            );
                            if r.clicked() {
                                run = Some(i);
                            }
                        }
                        if shown.is_empty() {
                            ui.label(RichText::new("Nothing matches").color(t.text_faint));
                        }
                    });
            });
        });
        if enter && let Some(&i) = shown.get(palette.selected) {
            run = Some(i);
        }
        if resp.response.clicked_elsewhere()
            && ctx.input(|i| i.pointer.any_pressed())
            && run.is_none()
        {
            close = true;
        }
        if close || run.is_some() {
            self.palette = None;
        }
        if let Some(i) = run
            && let Some((_, _, entry)) = entries.into_iter().nth(i)
        {
            self.run_entry(entry, ctx);
        }
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let t = Tokens::get(ctx);
        let frame = egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.field_border))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(18))
            .shadow(ctx.global_style().visuals.window_shadow);
        let mut close = false;
        let resp = egui::Modal::new(egui::Id::new("settings"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(110))
            .show(ctx, |ui| {
                ui.set_width(560.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Settings").font(theme::semibold(16.0)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::small_button(ui, "x", "Close").clicked() {
                            close = true;
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .max_height(560.0)
                    .show(ui, |ui| {
                        if self.settings.ui(ui) {
                            self.settings.set(ui.ctx());
                        }
                    });
            });
        if close || resp.should_close() {
            self.settings_open = false;
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        let now = std::time::Instant::now();
        self.toasts.retain(|t| t.until > now);
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        let t = Tokens::get(ctx);
        egui::Area::new(egui::Id::new("toasts"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-16.0, -16.0))
            .show(ctx, |ui| {
                for toast in &self.toasts {
                    egui::Frame::new()
                        .fill(t.card)
                        .stroke(Stroke::new(
                            1.0,
                            if toast.error { t.danger } else { t.success },
                        ))
                        .corner_radius(t.radius_lg)
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .shadow(ctx.global_style().visuals.popup_shadow)
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.horizontal(|ui| {
                                let icon = if toast.error { "circle-alert" } else { "check" };
                                icons::inline(
                                    ui,
                                    icon,
                                    16.0,
                                    if toast.error { t.danger } else { t.success },
                                );
                                ui.add(egui::Label::new(&toast.text).wrap());
                            });
                        });
                    ui.add_space(6.0);
                }
            });
    }
}

/// `needle`'s characters appear in `haystack` in order ("cmain" → "Check out main").
fn fuzzy(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|n| chars.any(|h| h == n))
}

/// A desktop notification, on its own thread (it can take a moment to show).
fn notify(text: &str) {
    let text = text.to_owned();
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .appname("gitcraft")
            .summary("gitcraft")
            .body(&text)
            .show();
    });
}

#[cfg(test)]
mod tests {
    use super::fuzzy;

    #[test]
    fn fuzzy_matching() {
        assert!(fuzzy("check out main", "cmain"));
        assert!(fuzzy("check out main", "co main"));
        assert!(!fuzzy("fetch", "push"));
        assert!(fuzzy("anything", ""));
    }
}
