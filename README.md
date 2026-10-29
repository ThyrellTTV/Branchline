# Branchline

A lightweight Git desktop client for Windows with a visual commit graph, branch management, file diffs, and an integrated terminal.

Based on **[GitCraft](https://github.com/Vancete/gitcraft)** by Iván. Branchline preserves its compact, dark, three-panel interface while establishing an independent app identity and development direction. See [UPSTREAM.md](UPSTREAM.md) for provenance and the changes in this derivative.

![Upstream GitCraft interface that Branchline is based on](docs/screenshot.png)

*Upstream screenshot; Branchline's initial interface is inherited from this design.*

## Status

Initial source version: **0.1.0**, based on GitCraft 0.3.0. The Git engine and interface are inherited from upstream. Branchline adds independent naming, application settings, release links, Windows packaging and upstream credit. Inherited features have not all been independently validated.

The [Windows build workflow](https://github.com/ThyrellTTV/Branchline/actions/workflows/windows.yml) runs the tests, compiles the app, and creates a portable ZIP. Check the run result before downloading its artifact. There is no published Branchline release yet.

## Included capabilities

- Open, initialize and clone repositories; multiple repository tabs.
- Colored commit graph with branch/tag references and history search.
- Local branches, remotes, worktrees, stashes and tags.
- File and partial staging, commit/amend, unified and side-by-side diffs.
- Merge/conflict tools, history, blame and advanced Git operations.
- Integrated PowerShell and other installed shells.
- Dark, gray and light themes, configurable zoom and keyboard shortcuts.

These capabilities come from upstream. The app uses the installed Git CLI, including its configured identities, credential helpers and hooks. GitHub/GitLab pull-request features additionally depend on `gh`/`glab` where used.

## Run on Windows

1. Install [Git for Windows](https://git-scm.com/download/win) and make sure `git --version` works.
2. Open a successful Windows workflow run and download the `Branchline-windows-x64` artifact.
3. Extract the artifact and the portable ZIP inside it, then run `branchline.exe`.

Keep the license and attribution files with the portable distribution. Start with a disposable repository when evaluating an early build.

You can also pass a repository path:

```powershell
.\branchline.exe "C:\path\to\repository"
```

## Build from source

Requirements: Git, Rust **1.95 or newer**, and the Microsoft C++ build tools with a Windows SDK. Use the MSVC Rust toolchain on Windows.

```powershell
git clone git@github.com:ThyrellTTV/Branchline.git
cd Branchline
cargo test --release --locked
cargo build --release --locked
.\target\release\branchline.exe
```

Branchline stores its UI state separately from GitCraft. The inherited macOS/Linux packaging sources remain available, but current CI validates Windows only.

## Development

See [ROADMAP.md](ROADMAP.md). Keep changes focused, add regression tests for behavioral changes, and run tests before submitting a pull request. The original commit history is retained so upstream fixes can be reviewed and merged.

## License and credits

Source: **MIT OR Apache-2.0**, inherited from GitCraft. Original copyright notices remain in [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

Fonts and icons retain their respective licenses; see [ATTRIBUTION.md](ATTRIBUTION.md). The initial app icon is inherited from GitCraft. Branchline is an independent derivative and does not imply endorsement by the upstream author.
