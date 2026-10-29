# Branchline roadmap

The initial goal is a Windows desktop client for local Git repositories, retaining GitCraft's compact dark interface: branches and worktrees on the left, a colored commit graph in the middle, changes and commits on the right, and an integrated terminal below.

## 0.1 — establish the foundation

- [x] Preserve upstream source, history, licenses and attribution.
- [x] Establish Branchline application and saved-state identity.
- [x] Add a Windows test/build/package workflow.
- [ ] Confirm the first CI build and tests pass.
- [ ] Smoke-test the executable with a disposable local repository.
- [ ] Review the UI at normal and high Windows display scaling.

## Next design decisions

These are candidates for discussion, not implemented features:

- Distinct Branchline icon and visual refinements while keeping the screenshot's density.
- Faster access to recent repositories and worktrees.
- Clearer staging and commit feedback for daily use.
- Review the inherited advanced Git operations and recovery behavior with fixture repositories.

## Release gate

Before publishing an executable release, require passing CI, a local Git workflow smoke test, a checked portable package, and a review of remaining upstream behavior. Do not describe inherited capabilities as individually verified until tested.
