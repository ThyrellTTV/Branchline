//! Thin wrapper over the `git` command line. Every call shells out to the installed git, so
//! gitcraft behaves exactly like the user's own git (config, credentials, hooks).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Field and record separators for `--format` strings (unit/record separator control chars).
const FS: char = '\x1f';
const RS: char = '\x1e';

/// How many commits the graph loads at a time; scrolling to the end loads the next page.
pub const LOG_PAGE: usize = 1500;

fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    // Never block on an interactive prompt: credential helpers with a GUI still work.
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    // `rebase --continue`, `merge --continue`… would otherwise open an editor nobody sees.
    cmd.env("GIT_EDITOR", "true");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Runs git in `dir` and returns stdout, or the error output.
pub fn run(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    run_ok_codes(dir, args, &[0])
}

/// Runs git and returns stdout as raw bytes (file contents: images…).
pub fn run_bytes(dir: Option<&Path>, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = git_cmd();
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args);
    let out = cmd
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// Like [`run`], but exit codes in `ok` count as success (`diff --no-index` exits 1 on changes).
pub fn run_ok_codes(dir: Option<&Path>, args: &[&str], ok: &[i32]) -> Result<String, String> {
    let mut cmd = git_cmd();
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args);
    let out = cmd
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if out.status.code().is_some_and(|c| ok.contains(&c)) {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        // Merges and pulls report conflicts on stdout and the summary on stderr.
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let err = match (stderr.is_empty(), stdout.is_empty()) {
            (true, true) => format!("git {} failed ({})", args.join(" "), out.status),
            (false, true) => stderr,
            (true, false) => stdout,
            (false, false) => format!("{stderr}\n{stdout}"),
        };
        Err(err)
    }
}

/// `git clone url dest`, run from `dest`'s parent.
pub fn clone(url: &str, dest: &Path) -> Result<(), String> {
    let dest = dest.to_string_lossy();
    run(None, &["clone", "--", url, &dest]).map(|_| ())
}

/// The folder name `git clone` would pick for `url`.
pub fn clone_dir_name(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\', ':']).next().unwrap_or_default();
    last.strip_suffix(".git").unwrap_or(last).to_owned()
}

/// `git --version`, or an error if git isn't installed / not on PATH.
pub fn version() -> Result<String, String> {
    run(None, &["--version"]).map(|s| s.trim().to_owned())
}

/// The top-level folder of the repository containing `path`.
pub fn toplevel(path: &Path) -> Result<PathBuf, String> {
    let out = run(Some(path), &["rev-parse", "--show-toplevel"])
        .map_err(|_| format!("{} is not inside a git repository", path.display()))?;
    Ok(PathBuf::from(out.trim()))
}

pub fn init(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("could not create folder: {e}"))?;
    run(Some(path), &["init"]).map(|_| ())
}

#[derive(Clone, Debug)]
pub struct Branch {
    /// Short name, e.g. `main` or `origin/main`.
    pub name: String,
    pub target: String,
    pub is_head: bool,
    pub upstream: String,
    /// e.g. `[ahead 1, behind 2]`.
    pub track: String,
}

impl Branch {
    /// Commits ahead of / behind the upstream, from `[ahead 1, behind 2]`.
    pub fn ahead_behind(&self) -> (u32, u32) {
        let inner = self.track.trim_start_matches('[').trim_end_matches(']');
        let mut result = (0, 0);
        for part in inner.split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                result.0 = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                result.1 = n.parse().unwrap_or(0);
            }
        }
        result
    }

    /// The upstream branch was deleted on the remote.
    pub fn upstream_gone(&self) -> bool {
        self.track == "[gone]"
    }
}

#[derive(Clone, Debug, Default)]
pub struct Refs {
    pub local: Vec<Branch>,
    /// Remote name → branches of that remote.
    pub remotes: Vec<(String, Vec<Branch>)>,
    pub tags: Vec<Branch>,
    pub head: String,
    /// Current branch name, or `None` when HEAD is detached / unborn.
    pub current: Option<String>,
}

pub fn refs(repo: &Path) -> Result<Refs, String> {
    let format = format!(
        "%(refname){FS}%(refname:short){FS}%(objectname){FS}%(HEAD){FS}%(upstream:short){FS}%(upstream:track){FS}%(*objectname)"
    );
    let out = run(
        Some(repo),
        &[
            "for-each-ref",
            &format!("--format={format}"),
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    let mut refs = Refs::default();
    for line in out.lines() {
        let f: Vec<&str> = line.split(FS).collect();
        let [full, short, oid, head, upstream, track, peeled] = f[..] else {
            continue;
        };
        // Annotated tags point at a tag object; the commit is the peeled id.
        let target = if peeled.is_empty() { oid } else { peeled };
        let branch = Branch {
            name: short.to_owned(),
            target: target.to_owned(),
            is_head: head == "*",
            upstream: upstream.to_owned(),
            track: track.to_owned(),
        };
        if full.starts_with("refs/heads/") {
            if branch.is_head {
                refs.current = Some(branch.name.clone());
            }
            refs.local.push(branch);
        } else if let Some(rest) = full.strip_prefix("refs/remotes/") {
            if rest.ends_with("/HEAD") {
                continue;
            }
            let remote = rest.split('/').next().unwrap_or_default().to_owned();
            match refs.remotes.iter_mut().find(|(r, _)| *r == remote) {
                Some((_, list)) => list.push(branch),
                None => refs.remotes.push((remote, vec![branch])),
            }
        } else if full.starts_with("refs/tags/") {
            refs.tags.push(branch);
        }
    }
    refs.head = run(Some(repo), &["rev-parse", "--verify", "-q", "HEAD"])
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    Ok(refs)
}

#[derive(Clone, Debug, Default)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Author time, seconds since the Unix epoch.
    pub time: i64,
    pub date: String,
    pub summary: String,
    /// Decorations (`HEAD -> main`, `origin/main`, `tag: v1`), as git prints them.
    pub refs: Vec<String>,
}

/// The commits of every branch (stashes aside), newest first, in an order fit for drawing the
/// graph: `limit` of them after the first `skip`.
pub fn log(repo: &Path, skip: usize, limit: usize) -> Result<Vec<Commit>, String> {
    let format = format!("--format=%H{FS}%P{FS}%an{FS}%ae{FS}%at{FS}%ad{FS}%s{FS}%D{RS}");
    let limit = format!("--max-count={limit}");
    let skip = format!("--skip={skip}");
    let out = match run(
        Some(repo),
        &[
            "log",
            "--exclude=refs/stash",
            "--all",
            "--date-order",
            "--date=format-local:%Y-%m-%d %H:%M",
            &skip,
            &limit,
            &format,
        ],
    ) {
        Ok(out) => out,
        // A freshly initialised repository has no commits yet.
        Err(e) if e.contains("does not have any commits") => String::new(),
        Err(e) => return Err(e),
    };
    let mut commits = Vec::new();
    for record in out.split(RS) {
        let record = record.trim_start_matches('\n');
        if record.is_empty() {
            continue;
        }
        let f: Vec<&str> = record.split(FS).collect();
        let [hash, parents, author, email, time, date, summary, decor] = f[..] else {
            continue;
        };
        commits.push(Commit {
            hash: hash.to_owned(),
            parents: parents.split_whitespace().map(str::to_owned).collect(),
            author: author.to_owned(),
            email: email.to_owned(),
            time: time.parse().unwrap_or(0),
            date: date.to_owned(),
            summary: summary.to_owned(),
            refs: decor
                .split(", ")
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        });
    }
    Ok(commits)
}

#[derive(Clone, Debug)]
pub struct ChangedFile {
    /// `A`, `M`, `D`, `R`, `C`, `T`…
    pub status: char,
    pub path: String,
    /// Original path for renames and copies.
    pub old_path: Option<String>,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct CommitDetail {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub author_date: String,
    pub committer: String,
    pub commit_date: String,
    pub message: String,
    pub files: Vec<ChangedFile>,
}

pub fn commit_detail(repo: &Path, hash: &str) -> Result<CommitDetail, String> {
    let format = format!("--format=%H{FS}%P{FS}%an{FS}%ae{FS}%ad{FS}%cn{FS}%cd{FS}%B");
    let out = run(
        Some(repo),
        &[
            "show",
            "-s",
            "--date=format-local:%Y-%m-%d %H:%M:%S",
            &format,
            hash,
        ],
    )?;
    let f: Vec<&str> = out.splitn(8, FS).collect();
    let [h, parents, author, email, adate, committer, cdate, message] = f[..] else {
        return Err("unexpected output from git show".to_owned());
    };

    // Merges are shown against their first parent, like most GUIs do.
    let diff_args = |kind: &'static str| {
        [
            "show",
            "--format=",
            "--diff-merges=first-parent",
            "-M",
            kind,
            "-z",
            hash,
        ]
    };
    let status_out = run(Some(repo), &diff_args("--name-status"))?;
    let numstat_out = run(Some(repo), &diff_args("--numstat"))?;

    let mut files = parse_name_status(&status_out);
    for (path, added, removed) in parse_numstat(&numstat_out) {
        if let Some(file) = files.iter_mut().find(|f| f.path == path) {
            file.added = added;
            file.removed = removed;
        }
    }

    Ok(CommitDetail {
        hash: h.to_owned(),
        parents: parents.split_whitespace().map(str::to_owned).collect(),
        author: author.to_owned(),
        email: email.to_owned(),
        author_date: adate.to_owned(),
        committer: committer.to_owned(),
        commit_date: cdate.to_owned(),
        message: message.trim_end().to_owned(),
        files,
    })
}

/// `--name-status -z`: `STATUS\0path\0` or, for renames/copies, `R100\0old\0new\0`.
fn parse_name_status(out: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut it = out.split('\0').filter(|s| !s.is_empty());
    while let Some(status) = it.next() {
        let code = status.chars().next().unwrap_or('?');
        let Some(first) = it.next() else { break };
        if matches!(code, 'R' | 'C') {
            let Some(second) = it.next() else { break };
            files.push(ChangedFile {
                status: code,
                path: second.to_owned(),
                old_path: Some(first.to_owned()),
                added: None,
                removed: None,
            });
        } else {
            files.push(ChangedFile {
                status: code,
                path: first.to_owned(),
                old_path: None,
                added: None,
                removed: None,
            });
        }
    }
    files
}

/// `--numstat -z`: `add\tdel\tpath\0` or, for renames, `add\tdel\t\0old\0new\0`.
fn parse_numstat(out: &str) -> Vec<(String, Option<u32>, Option<u32>)> {
    let mut result = Vec::new();
    let mut it = out.split('\0');
    while let Some(entry) = it.next() {
        let entry = entry.trim_start_matches('\n');
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let path = if path.is_empty() {
            // Rename: old path, then new path.
            let _old = it.next();
            it.next().unwrap_or_default().to_owned()
        } else {
            path.to_owned()
        };
        // Binary files report `-`.
        result.push((path, a.parse().ok(), d.parse().ok()));
    }
    result
}

/// The patch of one file in a commit (`ignore_ws`: `-w`).
pub fn file_diff(repo: &Path, hash: &str, path: &str, ignore_ws: bool) -> Result<String, String> {
    let mut args = vec!["show", "--format=", "--diff-merges=first-parent", "-M"];
    if ignore_ws {
        args.push("-w");
    }
    args.extend([hash, "--", path]);
    run(Some(repo), &args)
}

/// Which side of the index a working-tree file is listed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Staged,
    Unstaged,
    Untracked,
    Conflicted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusFile {
    pub path: String,
    pub old_path: Option<String>,
    /// Porcelain status letter for this side (`M`, `A`, `D`, `R`, `?`, `U`…).
    pub code: char,
    pub area: Area,
}

/// An operation git left half done (conflicts to resolve).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InProgress {
    Merge,
    CherryPick,
    Revert,
    Rebase,
}

impl InProgress {
    pub fn label(self) -> &'static str {
        match self {
            InProgress::Merge => "Merge",
            InProgress::CherryPick => "Cherry-pick",
            InProgress::Revert => "Revert",
            InProgress::Rebase => "Rebase",
        }
    }

    /// The git subcommand whose `--abort` cancels it.
    pub fn command(self) -> &'static str {
        match self {
            InProgress::Merge => "merge",
            InProgress::CherryPick => "cherry-pick",
            InProgress::Revert => "revert",
            InProgress::Rebase => "rebase",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct WorkStatus {
    pub staged: Vec<StatusFile>,
    pub unstaged: Vec<StatusFile>,
    pub conflicted: Vec<StatusFile>,
    pub in_progress: Option<InProgress>,
    /// Message git prepared for the pending merge / cherry-pick / revert commit.
    pub prepared_message: Option<String>,
}

impl WorkStatus {
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.conflicted.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: String,
    /// Short branch name, `None` when detached.
    pub branch: Option<String>,
    pub bare: bool,
    /// Lock reason (may be empty) when locked.
    pub locked: Option<String>,
    /// Why git would prune it (its folder is gone…).
    pub prunable: Option<String>,
    /// The main worktree (the first one git lists).
    pub main: bool,
    /// The worktree this tab shows.
    pub current: bool,
}

impl Worktree {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

pub fn worktrees(repo: &Path) -> Result<Vec<Worktree>, String> {
    let out = run(Some(repo), &["worktree", "list", "--porcelain", "-z"])?;
    let mut list = parse_worktrees(&out);
    let here = std::fs::canonicalize(repo).ok();
    for wt in &mut list {
        wt.current = here.is_some() && std::fs::canonicalize(&wt.path).ok() == here;
    }
    Ok(list)
}

/// `worktree list --porcelain -z`: `key value\0` lines, an empty line between worktrees.
fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut list: Vec<Worktree> = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in out.split('\0') {
        if line.is_empty() {
            list.extend(current.take());
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key == "worktree" {
            list.extend(current.take());
            current = Some(Worktree {
                path: PathBuf::from(value),
                main: list.is_empty(),
                ..Worktree::default()
            });
            continue;
        }
        let Some(wt) = current.as_mut() else { continue };
        match key {
            "HEAD" => wt.head = value.to_owned(),
            "branch" => {
                wt.branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_owned(),
                )
            }
            "bare" => wt.bare = true,
            "locked" => wt.locked = Some(value.to_owned()),
            "prunable" => wt.prunable = Some(value.to_owned()),
            _ => {}
        }
    }
    list.extend(current);
    list
}

pub fn status(repo: &Path) -> Result<WorkStatus, String> {
    let out = run(
        Some(repo),
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let mut status = parse_status(&out);
    for dir in ["rebase-merge", "rebase-apply"] {
        if let Ok(path) = run(Some(repo), &["rev-parse", "--git-path", dir])
            && repo.join(path.trim()).is_dir()
        {
            status.in_progress = Some(InProgress::Rebase);
        }
    }
    for (state, head) in [
        (InProgress::Merge, "MERGE_HEAD"),
        (InProgress::CherryPick, "CHERRY_PICK_HEAD"),
        (InProgress::Revert, "REVERT_HEAD"),
    ] {
        if status.in_progress.is_none()
            && run(Some(repo), &["rev-parse", "-q", "--verify", head]).is_ok()
        {
            status.in_progress = Some(state);
            break;
        }
    }
    if status.in_progress.is_some()
        && let Ok(path) = run(Some(repo), &["rev-parse", "--git-path", "MERGE_MSG"])
    {
        let path = repo.join(path.trim());
        status.prepared_message = std::fs::read_to_string(path).ok().map(|m| {
            // Drop git's `#` comment lines.
            m.lines()
                .filter(|l| !l.starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_owned()
        });
    }
    Ok(status)
}

/// `status --porcelain=v1 -z`: `XY path\0`, renames and copies `XY new\0old\0`.
fn parse_status(out: &str) -> WorkStatus {
    let mut status = WorkStatus::default();
    let mut it = out.split('\0');
    while let Some(entry) = it.next() {
        let mut chars = entry.chars();
        let (Some(x), Some(y), Some(' ')) = (chars.next(), chars.next(), chars.next()) else {
            continue;
        };
        let path = chars.as_str().to_owned();
        let old_path = if matches!(x, 'R' | 'C') {
            it.next().map(str::to_owned)
        } else {
            None
        };
        let file = |code, area| StatusFile {
            path: path.clone(),
            old_path: old_path.clone(),
            code,
            area,
        };
        let conflicted = x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D');
        if conflicted {
            status.conflicted.push(file('U', Area::Conflicted));
        } else if x == '?' {
            status.unstaged.push(file('?', Area::Untracked));
        } else if x == '!' {
            continue;
        } else {
            if x != ' ' {
                status.staged.push(file(x, Area::Staged));
            }
            if y != ' ' {
                status.unstaged.push(file(y, Area::Unstaged));
            }
        }
    }
    status
}

/// The patch of one uncommitted file (`ignore_ws`: `-w`).
pub fn work_diff(repo: &Path, file: &StatusFile, ignore_ws: bool) -> Result<String, String> {
    let path = file.path.as_str();
    let mut args: Vec<&str> = match file.area {
        Area::Staged => vec!["diff", "--cached", "-M"],
        _ => vec!["diff"],
    };
    if ignore_ws {
        args.push("-w");
    }
    args.extend(["--", path]);
    match file.area {
        Area::Staged | Area::Unstaged | Area::Conflicted => run(Some(repo), &args),
        // An untracked file has no history: show it as all added.
        Area::Untracked => run_ok_codes(
            Some(repo),
            &["diff", "--no-index", "--", "/dev/null", path],
            &[0, 1],
        ),
    }
}

/// The contents of `path` at `rev` (`HEAD`, a hash, or `""` for the index), or from disk when
/// `rev` is `None`.
pub fn file_at(repo: &Path, rev: Option<&str>, path: &str) -> Result<Vec<u8>, String> {
    match rev {
        None => std::fs::read(repo.join(path)).map_err(|e| format!("{path}: {e}")),
        Some(rev) => run_bytes(Some(repo), &["show", &format!("{rev}:{path}")]),
    }
}

/// HEAD's commit hash, or empty before the first commit.
pub fn head(repo: &Path) -> String {
    run(Some(repo), &["rev-parse", "--verify", "-q", "HEAD"])
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

/// The full message of HEAD (to amend it).
pub fn head_message(repo: &Path) -> Result<String, String> {
    run(Some(repo), &["log", "-1", "--format=%B"]).map(|s| s.trim_end().to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{0}`.
    pub name: String,
    pub hash: String,
    pub message: String,
    pub time: i64,
}

pub fn stashes(repo: &Path) -> Result<Vec<Stash>, String> {
    let out = run(
        Some(repo),
        &[
            "stash",
            "list",
            &format!("--format=%gd{FS}%H{FS}%gs{FS}%at"),
        ],
    )?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(FS).collect();
            let [name, hash, message, time] = f[..] else {
                return None;
            };
            // `On main: wip` / `WIP on main: 1234 msg` → the part after the colon reads better.
            let message = message
                .split_once(": ")
                .map(|(_, m)| m)
                .unwrap_or(message)
                .to_owned();
            Some(Stash {
                name: name.to_owned(),
                hash: hash.to_owned(),
                message,
                time: time.parse().unwrap_or(0),
            })
        })
        .collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

pub fn remotes(repo: &Path) -> Result<Vec<Remote>, String> {
    let out = run(Some(repo), &["remote", "-v"])?;
    let mut list: Vec<Remote> = Vec::new();
    for line in out.lines() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(url)) = (parts.next(), parts.next()) else {
            continue;
        };
        if !list.iter().any(|r| r.name == name) {
            list.push(Remote {
                name: name.to_owned(),
                url: url.to_owned(),
            });
        }
    }
    Ok(list)
}

/// Commits whose message or author matches `query` (case-insensitive), or whose hash starts
/// with it, newest first.
pub fn search(repo: &Path, query: &str) -> Result<Vec<String>, String> {
    let mut hashes: Vec<String> = Vec::new();
    let base = [
        "log",
        "--exclude=refs/stash",
        "--all",
        "--date-order",
        "-i",
        "--format=%H",
    ];
    for filter in [format!("--grep={query}"), format!("--author={query}")] {
        let mut args = base.to_vec();
        args.push("--fixed-strings");
        args.push(&filter);
        if let Ok(out) = run(Some(repo), &args) {
            for h in out.lines() {
                if !hashes.iter().any(|x| x == h) {
                    hashes.push(h.to_owned());
                }
            }
        }
    }
    let q = query.trim();
    if q.len() >= 4
        && q.chars().all(|c| c.is_ascii_hexdigit())
        && let Ok(h) = run(
            Some(repo),
            &["rev-parse", "--verify", "-q", &format!("{q}^{{commit}}")],
        )
    {
        hashes.insert(0, h.trim().to_owned());
    }
    Ok(hashes)
}

/// The name and email commits are made with here, and whether they're set for this repository
/// only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub email: String,
    pub local: bool,
}

pub fn identity(repo: &Path) -> Identity {
    let get = |args: &[&str]| run(Some(repo), args).map(|s| s.trim().to_owned()).ok();
    Identity {
        name: get(&["config", "user.name"]).unwrap_or_default(),
        email: get(&["config", "user.email"]).unwrap_or_default(),
        local: get(&["config", "--local", "user.name"]).is_some(),
    }
}

/// "3 minutes ago", "2 days ago"… for `time` seen from `now` (both Unix seconds).
pub fn relative_time(time: i64, now: i64) -> String {
    let secs = (now - time).max(0);
    let (n, unit) = match secs {
        0..60 => return "just now".to_owned(),
        60..3_600 => (secs / 60, "minute"),
        3_600..86_400 => (secs / 3_600, "hour"),
        86_400..604_800 => (secs / 86_400, "day"),
        604_800..2_629_800 => (secs / 604_800, "week"),
        2_629_800..31_557_600 => (secs / 2_629_800, "month"),
        _ => (secs / 31_557_600, "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_sides_renames_and_conflicts() {
        let s =
            parse_status("MM both.rs\0R  new.rs\0old.rs\0?? fresh.txt\0UU clash.rs\0 D gone.rs\0");
        let staged: Vec<_> = s.staged.iter().map(|f| (f.code, f.path.as_str())).collect();
        assert_eq!(staged, vec![('M', "both.rs"), ('R', "new.rs")]);
        assert_eq!(s.staged[1].old_path.as_deref(), Some("old.rs"));
        let unstaged: Vec<_> = s
            .unstaged
            .iter()
            .map(|f| (f.code, f.path.as_str()))
            .collect();
        assert_eq!(
            unstaged,
            vec![('M', "both.rs"), ('?', "fresh.txt"), ('D', "gone.rs")]
        );
        assert_eq!(s.conflicted.len(), 1);
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative_time(1000, 1030), "just now");
        assert_eq!(relative_time(0, 60), "1 minute ago");
        assert_eq!(relative_time(0, 2 * 3_600), "2 hours ago");
        assert_eq!(relative_time(0, 5 * 86_400), "5 days ago");
        assert_eq!(relative_time(0, 3 * 604_800), "3 weeks ago");
        assert_eq!(relative_time(0, 400 * 86_400), "1 year ago");
        assert_eq!(relative_time(100, 50), "just now");
    }

    #[test]
    fn worktree_list() {
        let out = "worktree C:/src/app\0HEAD abc\0branch refs/heads/main\0\0\
                   worktree C:/src/app-fix\0HEAD def\0detached\0locked moving disks\0\0\
                   worktree C:/gone\0HEAD 123\0branch refs/heads/old\0prunable gitdir file points to non-existent location\0\0";
        let list = parse_worktrees(out);
        assert_eq!(list.len(), 3);
        assert!(list[0].main && !list[1].main);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[1].branch, None);
        assert_eq!(list[1].locked.as_deref(), Some("moving disks"));
        assert!(list[2].prunable.is_some());
        assert_eq!(list[1].name(), "app-fix");
    }

    #[test]
    fn clone_folder_names() {
        assert_eq!(clone_dir_name("https://github.com/a/b.git"), "b");
        assert_eq!(clone_dir_name("git@github.com:a/b.git"), "b");
        assert_eq!(clone_dir_name("C:\\repos\\thing\\"), "thing");
        assert_eq!(clone_dir_name("git@host:proj"), "proj");
    }

    #[test]
    fn name_status_with_rename() {
        let files = parse_name_status("M\0a.txt\0R090\0old.rs\0new.rs\0D\0gone\0");
        assert_eq!(files.len(), 3);
        assert_eq!(files[1].status, 'R');
        assert_eq!(files[1].path, "new.rs");
        assert_eq!(files[1].old_path.as_deref(), Some("old.rs"));
    }

    #[test]
    fn numstat_with_rename_and_binary() {
        let stats = parse_numstat("1\t2\ta.txt\0-\t-\timg.png\0\n3\t0\t\0old.rs\0new.rs\0");
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0], ("a.txt".to_owned(), Some(1), Some(2)));
        assert_eq!(stats[1], ("img.png".to_owned(), None, None));
        assert_eq!(stats[2], ("new.rs".to_owned(), Some(3), Some(0)));
    }
}
