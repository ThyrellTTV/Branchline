//! One open repository: its state, background git work and the view
//! (branches on the left, commit graph or diff in the middle, commit detail / changes on the right).

mod detail;
mod dialogs;
mod diff;
mod graph_view;
mod sidebar;
pub(crate) mod widgets;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Key, Ui};

use crate::git::{self, Commit, CommitDetail, InProgress, Refs, StatusFile, WorkStatus, Worktree};
use crate::graph::{self, Graph};
use crate::icons;
use crate::terminal::Terminal;
use crate::theme::Tokens;

pub use dialogs::Dialog;
use diff::DiffLine;

/// Hash of the pseudo-commit that stands for uncommitted changes at the top of the graph.
pub(crate) const WIP_HASH: &str = "\u{0}wip";

/// Results coming back from background git calls.
enum Msg {
    Loaded(Result<Box<Loaded>, String>),
    Status(Result<WorkStatus, String>),
    Detail(String, Result<CommitDetail, String>),
    Diff(DiffKey, Result<String, String>),
    OpDone {
        label: String,
        result: Result<String, String>,
        refresh: Refresh,
    },
}

struct Loaded {
    refs: Refs,
    history: Vec<Commit>,
    work: WorkStatus,
    worktrees: Vec<Worktree>,
}

/// What to re-read after an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refresh {
    /// Only the working tree (staging changes nothing else).
    Status,
    All,
}

/// Which diff the central area shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DiffKey {
    Commit { hash: String, path: String },
    Work(StatusFile),
}

pub(crate) struct DiffView {
    key: DiffKey,
    lines: Option<Result<Vec<DiffLine>, String>>,
}

/// Everything the views ask for; applied after drawing so views only borrow the state.
pub(crate) enum Action {
    Select(usize),
    SelectHash(String),
    OpenDiff(DiffKey),
    CloseDiff,
    Ops {
        label: String,
        ops: Vec<Vec<String>>,
        refresh: Refresh,
    },
    Checkout(String),
    CheckoutRemote(String),
    Dialog(Dialog),
    Commit,
    Fetch,
    /// Open a folder (a worktree) in its own tab.
    OpenTab(PathBuf),
    ToggleTerminal,
    RestartTerminal,
    Pull,
    Push,
    Reload,
}

pub struct RepoTab {
    pub path: PathBuf,
    pub name: String,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,

    loading: bool,
    load_error: Option<String>,
    refs: Refs,
    /// Commits as git listed them.
    history: Vec<Commit>,
    work: WorkStatus,
    /// Rows of the graph: the WIP pseudo-commit (when the tree is dirty) followed by `history`.
    commits: Vec<Commit>,
    graph: Graph,
    index: HashMap<String, usize>,

    selected: Option<usize>,
    scroll_to: Option<usize>,
    detail: Option<CommitDetail>,
    detail_error: Option<String>,
    diff: Option<DiffView>,

    commit_summary: String,
    commit_body: String,
    dialog: Option<Dialog>,

    busy_op: Option<String>,
    status: Option<(String, bool)>,
    branch_filter: String,
    /// Jump to the changes row on the next rebuild (after an operation left conflicts).
    focus_wip: bool,
    worktrees: Vec<Worktree>,
    terminal: Option<Terminal>,
    terminal_open: bool,
    terminal_error: Option<String>,
    /// A folder the app should open in a new tab (taken by `App`).
    pub open_request: Option<PathBuf>,
}

impl RepoTab {
    pub fn new(path: PathBuf, ctx: &egui::Context) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let (tx, rx) = channel();
        let mut tab = Self {
            path,
            name,
            tx,
            rx,
            loading: false,
            load_error: None,
            refs: Refs::default(),
            history: Vec::new(),
            work: WorkStatus::default(),
            commits: Vec::new(),
            graph: Graph {
                rows: Vec::new(),
                width: 0,
            },
            index: HashMap::new(),
            selected: None,
            scroll_to: None,
            detail: None,
            detail_error: None,
            diff: None,
            commit_summary: String::new(),
            commit_body: String::new(),
            dialog: None,
            busy_op: None,
            status: None,
            branch_filter: String::new(),
            focus_wip: false,
            worktrees: Vec::new(),
            terminal: None,
            terminal_open: false,
            terminal_error: None,
            open_request: None,
        };
        tab.reload(ctx);
        tab
    }

    pub fn is_busy(&self) -> bool {
        self.loading || self.busy_op.is_some()
    }

    /// Re-reads refs, history and working-tree status in the background.
    pub fn reload(&mut self, ctx: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        let path = self.path.clone();
        spawn(&self.tx, ctx, move || {
            Msg::Loaded((|| {
                let refs = git::refs(&path)?;
                let history = git::log(&path)?;
                let work = git::status(&path)?;
                // Very old gits have no `worktree list`; the section just stays empty.
                let worktrees = git::worktrees(&path).unwrap_or_default();
                Ok(Box::new(Loaded {
                    refs,
                    history,
                    work,
                    worktrees,
                }))
            })())
        });
    }

    fn reload_status(&mut self, ctx: &egui::Context) {
        let path = self.path.clone();
        spawn(&self.tx, ctx, move || Msg::Status(git::status(&path)));
    }

    fn is_wip(&self, row: usize) -> bool {
        self.commits.get(row).is_some_and(|c| c.hash == WIP_HASH)
    }

    fn selected_hash(&self) -> Option<String> {
        self.selected
            .and_then(|r| self.commits.get(r))
            .map(|c| c.hash.clone())
    }

    /// Rebuilds the graph rows from `history` and `work`, keeping the selection.
    fn rebuild(&mut self, ctx: &egui::Context) {
        let previous = self.selected_hash();
        self.commits.clear();
        if !self.work.is_clean() {
            self.commits.push(Commit {
                hash: WIP_HASH.to_owned(),
                parents: if self.refs.head.is_empty() {
                    Vec::new()
                } else {
                    vec![self.refs.head.clone()]
                },
                author: String::new(),
                date: String::new(),
                summary: String::new(),
                refs: Vec::new(),
            });
        }
        self.commits.extend(self.history.iter().cloned());
        self.graph = graph::layout(&self.commits);
        self.index = self
            .commits
            .iter()
            .enumerate()
            .map(|(i, c)| (c.hash.clone(), i))
            .collect();

        // Keep the selection; otherwise start on the changes, or HEAD.
        let mut keep = previous.as_ref().and_then(|h| self.index.get(h).copied());
        if std::mem::take(&mut self.focus_wip) && self.index.contains_key(WIP_HASH) {
            keep = None;
        }
        let target = keep
            .or_else(|| self.index.get(WIP_HASH).copied())
            .or_else(|| self.index.get(&self.refs.head).copied());
        match target {
            Some(row) if keep == Some(row) => self.selected = Some(row),
            Some(row) => {
                self.selected = None;
                self.select(row, ctx);
                self.scroll_to = Some(row);
            }
            None => {
                self.selected = None;
                self.detail = None;
            }
        }

        // A merge or cherry-pick leaves a message to commit with.
        if let Some(msg) = &self.work.prepared_message
            && self.commit_summary.is_empty()
            && self.commit_body.is_empty()
        {
            let (summary, body) = msg.split_once('\n').unwrap_or((msg.as_str(), ""));
            self.commit_summary = summary.trim().to_owned();
            self.commit_body = body.trim().to_owned();
        }

        // The diff of a file that's no longer listed (staged, committed…) is stale.
        if let Some(DiffView {
            key: DiffKey::Work(file),
            ..
        }) = &self.diff
        {
            let still = self
                .work
                .staged
                .iter()
                .chain(&self.work.unstaged)
                .chain(&self.work.conflicted)
                .any(|f| f == file);
            if still {
                let key = DiffKey::Work(file.clone());
                self.diff = None;
                self.open_diff(key, ctx);
            } else {
                self.diff = None;
            }
        }
    }

    fn select(&mut self, row: usize, ctx: &egui::Context) {
        let Some(commit) = self.commits.get(row) else {
            return;
        };
        if self.selected == Some(row) {
            return;
        }
        self.selected = Some(row);
        self.detail = None;
        self.detail_error = None;
        if matches!(
            self.diff,
            Some(DiffView {
                key: DiffKey::Commit { .. },
                ..
            })
        ) || !self.is_wip(row)
        {
            self.diff = None;
        }
        if commit.hash == WIP_HASH {
            return;
        }
        let path = self.path.clone();
        let hash = commit.hash.clone();
        spawn(&self.tx, ctx, move || {
            let detail = git::commit_detail(&path, &hash);
            Msg::Detail(hash, detail)
        });
    }

    fn select_hash(&mut self, hash: &str, ctx: &egui::Context) {
        match self.index.get(hash).copied() {
            Some(row) => {
                self.select(row, ctx);
                self.scroll_to = Some(row);
            }
            None => {
                self.status = Some((
                    format!(
                        "Commit {} is not in the loaded history",
                        hash.get(..8).unwrap_or(hash)
                    ),
                    true,
                ));
            }
        }
    }

    /// Shows a diff in the central area; asking for the open one closes it.
    fn open_diff(&mut self, key: DiffKey, ctx: &egui::Context) {
        if self.diff.as_ref().is_some_and(|d| d.key == key) {
            self.diff = None;
            return;
        }
        self.diff = Some(DiffView {
            key: key.clone(),
            lines: None,
        });
        let path = self.path.clone();
        spawn(&self.tx, ctx, move || {
            let result = match &key {
                DiffKey::Commit { hash, path: file } => git::file_diff(&path, hash, file),
                DiffKey::Work(file) => git::work_diff(&path, file),
            };
            Msg::Diff(key, result)
        });
    }

    /// Runs git commands one after another (stopping at the first failure), then refreshes.
    fn run_ops(
        &mut self,
        label: &str,
        ops: Vec<Vec<String>>,
        refresh: Refresh,
        ctx: &egui::Context,
    ) {
        if self.busy_op.is_some() {
            return;
        }
        self.busy_op = Some(label.to_owned());
        self.status = None;
        let path = self.path.clone();
        let label = label.to_owned();
        spawn(&self.tx, ctx, move || {
            let mut result = Ok(String::new());
            for op in &ops {
                let args: Vec<&str> = op.iter().map(String::as_str).collect();
                result = git::run(Some(&path), &args);
                if result.is_err() {
                    break;
                }
            }
            Msg::OpDone {
                label,
                result,
                refresh,
            }
        });
    }

    fn checkout_remote(&mut self, remote_branch: &str, ctx: &egui::Context) {
        // `origin/feature` → local `feature` tracking it (or just switch if it exists).
        let local = remote_branch
            .split_once('/')
            .map(|(_, b)| b)
            .unwrap_or(remote_branch);
        let op = if self.refs.local.iter().any(|b| b.name == local) {
            ops(&[&["checkout", local]])
        } else {
            ops(&[&["checkout", "--track", remote_branch]])
        };
        self.run_ops(&format!("Checkout {local}"), op, Refresh::All, ctx);
    }

    fn push(&mut self, ctx: &egui::Context) {
        let Some(current) = self.refs.current.clone() else {
            self.status = Some(("Push needs a checked-out branch".to_owned(), true));
            return;
        };
        let has_upstream = self
            .refs
            .local
            .iter()
            .any(|b| b.name == current && !b.upstream.is_empty() && !b.upstream_gone());
        if has_upstream {
            self.run_ops("Push", ops(&[&["push"]]), Refresh::All, ctx);
            return;
        }
        // First push of a branch: publish it and track the remote one.
        let remote = self
            .refs
            .remotes
            .iter()
            .map(|(r, _)| r.as_str())
            .find(|r| *r == "origin")
            .or_else(|| self.refs.remotes.first().map(|(r, _)| r.as_str()))
            .map(str::to_owned);
        match remote {
            Some(remote) => self.run_ops(
                &format!("Push {current} to {remote}"),
                ops(&[&["push", "-u", &remote, &current]]),
                Refresh::All,
                ctx,
            ),
            None => {
                self.status = Some(("This repository has no remote to push to".to_owned(), true))
            }
        }
    }

    fn commit(&mut self, ctx: &egui::Context) {
        let summary = self.commit_summary.trim();
        if summary.is_empty() {
            self.status = Some(("Write a commit summary first".to_owned(), true));
            return;
        }
        if !self.work.conflicted.is_empty() {
            self.status = Some(("Resolve the conflicted files first".to_owned(), true));
            return;
        }
        let mut commit = vec!["commit".to_owned(), "-m".to_owned(), summary.to_owned()];
        let body = self.commit_body.trim();
        if !body.is_empty() {
            commit.extend(["-m".to_owned(), body.to_owned()]);
        }
        // Nothing staged: commit everything, like "Stage all and commit".
        let mut steps = Vec::new();
        if self.work.staged.is_empty() {
            steps.push(vec!["add".to_owned(), "-A".to_owned()]);
        }
        steps.push(commit);
        self.run_ops("Commit", steps, Refresh::All, ctx);
    }

    /// Drains finished background work.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded(result) => {
                    self.loading = false;
                    match result {
                        Ok(loaded) => {
                            self.load_error = None;
                            self.refs = loaded.refs;
                            self.history = loaded.history;
                            self.work = loaded.work;
                            self.worktrees = loaded.worktrees;
                            self.rebuild(ctx);
                        }
                        Err(e) => self.load_error = Some(e),
                    }
                }
                Msg::Status(result) => match result {
                    Ok(work) => {
                        self.work = work;
                        self.rebuild(ctx);
                    }
                    Err(e) => self.status = Some((e, true)),
                },
                Msg::Detail(hash, result) => {
                    if self.selected_hash().as_deref() == Some(hash.as_str()) {
                        match result {
                            Ok(d) => self.detail = Some(d),
                            Err(e) => self.detail_error = Some(e),
                        }
                    }
                }
                Msg::Diff(key, result) => {
                    if let Some(view) = &mut self.diff
                        && view.key == key
                    {
                        view.lines = Some(result.map(|text| diff::parse(&text)));
                    }
                }
                Msg::OpDone {
                    label,
                    result,
                    refresh,
                } => {
                    self.busy_op = None;
                    match result {
                        Ok(_) => {
                            if label == "Commit" {
                                self.commit_summary.clear();
                                self.commit_body.clear();
                            }
                            // Quiet success for staging, which happens all the time.
                            if refresh == Refresh::All {
                                self.status = Some((format!("{label}: done"), false));
                            }
                        }
                        Err(e) => {
                            self.focus_wip = e.contains("CONFLICT");
                            self.status = Some((format!("{label} failed: {e}"), true));
                        }
                    }
                    match refresh {
                        Refresh::Status => self.reload_status(ctx),
                        Refresh::All => self.reload(ctx),
                    }
                }
            }
        }
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Select(row) => self.select(row, ctx),
            Action::SelectHash(hash) => self.select_hash(&hash, ctx),
            Action::OpenDiff(key) => self.open_diff(key, ctx),
            Action::CloseDiff => self.diff = None,
            Action::Ops {
                label,
                ops,
                refresh,
            } => self.run_ops(&label, ops, refresh, ctx),
            Action::Checkout(name) => self.run_ops(
                &format!("Checkout {name}"),
                ops(&[&["checkout", &name]]),
                Refresh::All,
                ctx,
            ),
            Action::CheckoutRemote(name) => self.checkout_remote(&name, ctx),
            Action::Dialog(dialog) => self.dialog = Some(dialog),
            Action::Commit => self.commit(ctx),
            Action::Fetch => self.run_ops(
                "Fetch",
                ops(&[&["fetch", "--all", "--prune"]]),
                Refresh::All,
                ctx,
            ),
            Action::Pull => self.run_ops("Pull", ops(&[&["pull", "--ff-only"]]), Refresh::All, ctx),
            Action::Push => self.push(ctx),
            Action::Reload => self.reload(ctx),
            Action::OpenTab(path) => self.open_request = Some(path),
            Action::ToggleTerminal => {
                self.terminal_open = !self.terminal_open;
                if self.terminal_open && self.terminal.is_none() {
                    self.start_terminal(ctx);
                }
            }
            Action::RestartTerminal => {
                self.terminal = None;
                self.start_terminal(ctx);
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let t = Tokens::get(&ctx);
        let mut acts = Vec::new();

        if self.diff.is_some()
            && self.dialog.is_none()
            && !ctx.egui_wants_keyboard_input()
            && ctx.input(|i| i.key_pressed(Key::Escape))
        {
            acts.push(Action::CloseDiff);
        }

        egui::Panel::top("repo_toolbar")
            .frame(
                egui::Frame::new()
                    .fill(t.chrome)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .stroke(egui::Stroke::new(1.0, t.separator)),
            )
            .show(ui, |ui| self.toolbar(ui, &mut acts));
        let side = egui::Frame::new().fill(t.card);
        egui::Panel::left("repo_branches")
            .resizable(true)
            .default_size(260.0)
            .size_range(170.0..=520.0)
            .frame(side)
            .show(ui, |ui| self.sidebar(ui, &mut acts));
        egui::Panel::right("repo_detail")
            .resizable(true)
            .default_size(420.0)
            .size_range(280.0..=900.0)
            .frame(side)
            .show(ui, |ui| self.detail_panel(ui, &mut acts));
        if self.terminal_open {
            egui::Panel::bottom("repo_terminal")
                .resizable(true)
                .default_size(260.0)
                .size_range(90.0..=900.0)
                .frame(egui::Frame::new().fill(t.card))
                .show(ui, |ui| self.terminal_panel(ui, &mut acts));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.dock))
            .show(ui, |ui| {
                if self.diff.is_some() {
                    self.diff_view(ui, &mut acts);
                } else {
                    self.graph_view(ui, &mut acts);
                }
            });

        self.dialogs(&ctx, &mut acts);
        for action in acts {
            self.apply(action, &ctx);
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let idle = self.busy_op.is_none();
            let has_remote = !self.refs.remotes.is_empty();
            if icons::tool_button(ui, "cloud-download", "Fetch", idle && has_remote)
                .on_hover_text("Fetch all remotes (git fetch --all --prune)")
                .clicked()
            {
                acts.push(Action::Fetch);
            }
            if icons::tool_button(ui, "arrow-down-to-line", "Pull", idle && has_remote)
                .on_hover_text("Fast-forward the current branch (git pull --ff-only)")
                .clicked()
            {
                acts.push(Action::Pull);
            }
            if icons::tool_button(ui, "arrow-up-from-line", "Push", idle && has_remote)
                .on_hover_text("Push the current branch (publishes it if it has no upstream yet)")
                .clicked()
            {
                acts.push(Action::Push);
            }
            widgets::toolbar_separator(ui);
            let head = self.refs.head.clone();
            if icons::tool_button(ui, "git-branch-plus", "Branch", idle && !head.is_empty())
                .on_hover_text("Create a branch at HEAD")
                .clicked()
            {
                acts.push(Action::Dialog(Dialog::create_branch(&head, "HEAD")));
            }
            let terminal = icons::tool_button(ui, "square-terminal", "Terminal", true)
                .on_hover_text("Terminal in the repository folder");
            if self.terminal_open {
                ui.painter().line_segment(
                    [
                        terminal.rect.left_bottom() + egui::vec2(6.0, -1.0),
                        terminal.rect.right_bottom() + egui::vec2(-6.0, -1.0),
                    ],
                    egui::Stroke::new(2.0, t.accent),
                );
            }
            if terminal.clicked() {
                acts.push(Action::ToggleTerminal);
            }
            widgets::toolbar_separator(ui);
            if icons::tool_button(ui, "refresh-cw", "", !self.loading)
                .on_hover_text("Reload (F5)")
                .clicked()
            {
                acts.push(Action::Reload);
            }
            widgets::toolbar_separator(ui);

            match &self.refs.current {
                Some(branch) => {
                    icons::inline(ui, "git-branch", 14.0, t.text_dim);
                    ui.label(egui::RichText::new(branch).font(crate::theme::medium(12.5)));
                }
                None if self.refs.head.is_empty() => {
                    ui.label(egui::RichText::new("No commits yet").color(t.text_dim));
                }
                None => {
                    ui.label(
                        egui::RichText::new(format!(
                            "Detached HEAD at {}",
                            self.refs.head.get(..8).unwrap_or(&self.refs.head)
                        ))
                        .color(t.warning),
                    );
                }
            }

            if let Some(state) = self.work.in_progress {
                ui.add_space(10.0);
                widgets::chip(ui, &format!("{} in progress", state.label()), t.warning);
                if ui
                    .add_enabled(
                        idle,
                        egui::Button::new(format!("Abort {}", state.label().to_lowercase())),
                    )
                    .clicked()
                {
                    acts.push(abort_action(state));
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((msg, is_err)) = &self.status {
                    let color = if *is_err { t.danger } else { t.text_dim };
                    let first_line = headline(msg);
                    ui.add(
                        egui::Label::new(egui::RichText::new(first_line).color(color)).truncate(),
                    )
                    .on_hover_text(msg.as_str());
                }
                if let Some(op) = &self.busy_op {
                    ui.label(egui::RichText::new(format!("{op}…")).color(t.text_dim));
                    ui.spinner();
                } else if self.loading {
                    ui.spinner();
                }
            });
        });
    }
}

impl RepoTab {
    fn start_terminal(&mut self, ctx: &egui::Context) {
        match Terminal::spawn(&self.path, ctx) {
            Ok(term) => {
                self.terminal = Some(term);
                self.terminal_error = None;
            }
            Err(e) => self.terminal_error = Some(e),
        }
    }

    /// The bottom panel: a header strip and the shell.
    fn terminal_panel(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let title = match &self.terminal {
            Some(term) => format!("Terminal · {}", term.shell),
            None => "Terminal".to_owned(),
        };
        widgets::panel_header(ui, &title, |ui| {
            if icons::small_button(ui, "x", "Close the panel (the shell keeps running)").clicked() {
                acts.push(Action::ToggleTerminal);
            }
            if icons::small_button(ui, "refresh-cw", "Restart the shell").clicked() {
                acts.push(Action::RestartTerminal);
            }
        });
        if let Some(err) = &self.terminal_error {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.colored_label(t.danger, err);
            });
            return;
        }
        let Some(term) = &mut self.terminal else {
            return;
        };
        if term.has_exited() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("The shell has exited.").color(t.text_dim));
                if ui.button("Start a new one").clicked() {
                    acts.push(Action::RestartTerminal);
                }
            });
            return;
        }
        term.ui(ui);
    }
}

fn abort_action(state: InProgress) -> Action {
    Action::Ops {
        label: format!("Abort {}", state.label().to_lowercase()),
        ops: ops(&[&[state.command(), "--abort"]]),
        refresh: Refresh::All,
    }
}

/// The line of a git error worth showing in the toolbar: conflicts and `error:`/`fatal:` lines
/// beat progress chatter like "Auto-merging x".
fn headline(msg: &str) -> String {
    let (label, body) = msg.split_once(": ").unwrap_or(("", msg));
    let conflicts = body.lines().filter(|l| l.starts_with("CONFLICT")).count();
    if conflicts > 0 {
        let files = if conflicts == 1 { "file" } else { "files" };
        return format!("{label}: conflicts in {conflicts} {files}, resolve them in Changes");
    }
    let line = body
        .lines()
        .find(|l| l.starts_with("error:") || l.starts_with("fatal:"))
        .or_else(|| body.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or_default();
    if label.is_empty() {
        line.to_owned()
    } else {
        format!("{label}: {line}")
    }
}

/// `&[&["checkout", "main"]]` → owned argument lists.
pub(crate) fn ops(list: &[&[&str]]) -> Vec<Vec<String>> {
    list.iter()
        .map(|args| args.iter().map(|a| (*a).to_owned()).collect())
        .collect()
}

/// Runs `job` on a background thread and wakes the UI when its message is sent.
fn spawn(tx: &Sender<Msg>, ctx: &egui::Context, job: impl FnOnce() -> Msg + Send + 'static) {
    let tx = tx.clone();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(job());
        ctx.request_repaint();
    });
}
pub use widgets::primary_button;

#[cfg(test)]
mod tests {
    use super::headline;

    #[test]
    fn headline_prefers_conflicts_and_errors() {
        let merge = "Merge x failed: Auto-merging a.rs
CONFLICT (content): Merge conflict in a.rs
Automatic merge failed";
        assert_eq!(
            headline(merge),
            "Merge x failed: conflicts in 1 file, resolve them in Changes"
        );
        let push = "Push failed: To origin
 ! [rejected] main
error: failed to push some refs";
        assert_eq!(
            headline(push),
            "Push failed: error: failed to push some refs"
        );
        assert_eq!(headline("Fetch: done"), "Fetch: done");
    }
}
