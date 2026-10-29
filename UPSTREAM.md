# Upstream provenance

Branchline is a derivative of [GitCraft](https://github.com/Vancete/gitcraft), created by Iván (Vancete).

- Base commit: `e597ea560e0a55b44ddde872989ac4346bea0cb2`
- Upstream version: 0.3.0
- Upstream source license: MIT OR Apache-2.0
- Original README: [docs/UPSTREAM-README.md](docs/UPSTREAM-README.md)

The original Git history, copyright notices, both license files, and third-party asset attributions are retained. The initial Branchline version preserves GitCraft's Git engine, interface, fonts, and icons. Those features were developed upstream; this repository does not claim otherwise.

Initial Branchline changes:

- Independent application, executable, installer and saved-state identity.
- Branchline release checks and repository links.
- Visible upstream credit on the start page.
- Windows build/test and portable-package workflow.
- Documentation and a roadmap for this derivative.

Keep the upstream remote when developing locally:

```sh
git remote add upstream https://github.com/Vancete/gitcraft.git
git fetch upstream
```

Review and merge upstream updates deliberately. Branchline has its own version numbering starting at 0.1.0.
