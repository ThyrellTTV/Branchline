# gitcraft

A small, personal visual git client: a simplified take on the usual graph-based git GUIs.
Rust + [egui/eframe](https://github.com/emilk/egui), the same stack and look as WordCraft and
PhotoCraft, so it builds on Windows, Linux and macOS. It drives the installed `git` command line,
so your config, credentials and hooks apply as usual.

![gitcraft: branches and worktrees on the left, the graph with branch pills, uncommitted changes on the right and the integrated terminal below](docs/screenshot.png)

## Features

- **Start page**: checks that `git` is installed; open an existing repository, create one
  (`git init`) or clone one; recent repositories.
- **Tabs** like a browser, one per repository. `+` / Ctrl+T opens a start page, Ctrl+W closes,
  middle-click closes. Open tabs, recents and the theme are remembered.
- **Branches** (left): local branches with ahead/behind counts, remote branches per remote, tags,
  a filter. Click jumps to the commit; double-click checks out. Right-click: check out, merge into
  the current branch, create a branch, copy the name, delete (with force option), delete from the
  remote, delete a tag.
- **Worktrees** (left): every worktree with its branch, marked main / locked / missing. Add one
  (on an existing or a new branch) with `+`, double-click or *Open in a tab* to work in it,
  lock/unlock, remove (with force), prune missing ones. A branch checked out in another worktree
  shows a worktree icon, and double-clicking it opens that worktree. *Check out in a new
  worktree…* on a branch's menu.
- **Graph** (centre): every branch, coloured lanes, up to 5000 commits, ↑/↓ to move. Branch and
  tag pills sit in their own column left of the graph, joined to their commit: white text, a
  laptop icon when the branch is local, a cloud when a remote has it at that commit (`x` and
  `origin/x` together make one pill), the tip author's avatar, `+N` when more refs share the
  commit (hover lists them). An *Uncommitted changes*
  row sits on top when the working tree is dirty. Right-click a commit: create a branch, check it
  out (detached), cherry-pick, revert, copy hash or message.
- **Detail** (right): commit message, author, parents (clickable), changed files with +/−.
  On the changes row: conflicted / unstaged / staged lists with stage, unstage, discard and
  mark-resolved buttons on hover, *Stage all* / *Unstage all*, and the commit box
  (Ctrl+Enter; with nothing staged it stages everything).
- **Diff**: clicking a file shows its diff in the centre with old/new line numbers (Esc closes).
- **Merges and conflicts**: a merge, cherry-pick or revert that stops on conflicts shows a banner
  with *Abort*; the prepared merge message fills the commit box.
- **Toolbar**: Fetch (`--all --prune`), Pull (`--ff-only`), Push (publishes a new branch with
  `-u`), new branch, terminal, refresh (F5; also when the window regains focus).
- **Terminal**: the *Terminal* button opens your shell (PowerShell on Windows, `$SHELL` elsewhere)
  in the repository folder, in a resizable panel under the graph. It runs in a real
  pseudo-terminal (ConPTY on Windows), with colours, scrollback (mouse wheel) and keys such as
  Ctrl+C, arrows and Tab going to the shell. Closing the panel keeps the shell running.
- **Themes**: Dark, Medium Gray and Light, from PhotoCraft's Pro palettes.

## Build and run

```bash
cargo run --release                   # start
cargo run --release -- path/to/repo   # open a repository directly
cargo test
```

Linux needs the usual eframe system packages (e.g. `libxkbcommon-dev`, `libgtk-3-dev` for the
file dialogs).

### Looking at the UI headlessly

`examples/ui_shot.rs` drives the app without a window (egui_kittest) and saves PNGs:

```bash
cargo run --example ui_shot -- path/to/repo /tmp/shot click:500,266 shot key:Escape shot
```

Steps: `click:x,y`, `dclick:x,y`, `rclick:x,y`, `hover:x,y`, `leave`, `key:Name`, `type:text`,
`wait:ms`, `shot`. The README screenshot was made this way, on a demo repository.

## Layout

| File | Job |
|---|---|
| `src/git.rs` | `git` CLI wrapper and output parsing (refs, log, status, diffs) |
| `src/graph.rs` | lane layout for the commit graph |
| `src/theme.rs` | colour tokens, fonts, egui style (PhotoCraft's Pro themes) |
| `src/icons.rs` | embedded Lucide SVG icons, tinted |
| `src/repo/mod.rs` | one repository tab: state, background git work, toolbar |
| `src/repo/sidebar.rs` | branches, worktrees and tags pane |
| `src/repo/graph_view.rs` | commit graph and the branch/tag pills |
| `src/repo/detail.rs` | commit detail, changes and commit box |
| `src/repo/diff.rs` | diff parsing and the diff view |
| `src/repo/dialogs.rs` | create-branch, add-worktree and confirmation dialogs |
| `src/repo/widgets.rs` | shared painted widgets (panel headers, file rows, chips) |
| `src/terminal.rs` | integrated terminal: pty, VT parsing (`vt100`), painting, keys |
| `src/app.rs` | window shell: git check, tabs, start page, cloning, persistence |

Assets and their licences: [ATTRIBUTION.md](ATTRIBUTION.md).
