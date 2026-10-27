# gitcraft

A small, personal visual git client: a simplified take on the usual graph-based git GUIs.
Rust + [egui/eframe](https://github.com/emilk/egui), the same stack and look as WordCraft and
PhotoCraft, so it builds on Windows, Linux and macOS. It drives the installed `git` command line,
so your config, credentials and hooks apply as usual.

## Features

- **Start page**: checks that `git` is installed; open an existing repository, create one
  (`git init`) or clone one; recent repositories.
- **Tabs** like a browser, one per repository. `+` / Ctrl+T opens a start page, Ctrl+W closes,
  middle-click closes. Open tabs, recents and the theme are remembered.
- **Branches** (left): local branches with ahead/behind counts, remote branches per remote, tags,
  a filter. Click jumps to the commit; double-click checks out. Right-click: check out, merge into
  the current branch, create a branch, copy the name, delete (with force option), delete from the
  remote, delete a tag.
- **Graph** (centre): every branch, coloured lanes, ref badges (a local branch in sync with its
  remote shows one badge with a cloud), up to 5000 commits, ↑/↓ to move. An *Uncommitted changes*
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
  `-u`), new branch, refresh (F5; also when the window regains focus).
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

Steps: `click:x,y`, `rclick:x,y`, `hover:x,y`, `key:Name`, `type:text`, `wait:ms`, `shot`.

## Layout

| File | Job |
|---|---|
| `src/git.rs` | `git` CLI wrapper and output parsing (refs, log, status, diffs) |
| `src/graph.rs` | lane layout for the commit graph |
| `src/theme.rs` | colour tokens, fonts, egui style (PhotoCraft's Pro themes) |
| `src/icons.rs` | embedded Lucide SVG icons, tinted |
| `src/repo/mod.rs` | one repository tab: state, background git work, toolbar |
| `src/repo/sidebar.rs` | branches pane |
| `src/repo/graph_view.rs` | commit graph |
| `src/repo/detail.rs` | commit detail, changes and commit box |
| `src/repo/diff.rs` | diff parsing and the diff view |
| `src/repo/dialogs.rs` | create-branch and confirmation dialogs |
| `src/repo/widgets.rs` | shared painted widgets (panel headers, file rows, chips) |
| `src/app.rs` | window shell: git check, tabs, start page, cloning, persistence |

Assets and their licences: [ATTRIBUTION.md](ATTRIBUTION.md).
