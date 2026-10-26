# 0001: Public names

Status: decided for the first release, 2026-10-26.

The executable is **`rosetta`**, with `glossary`, `atlas`, `config` and `hooks` commands. The project is described as a repository glossary and map when search results need disambiguation from Apple Rosetta and other projects with the same name.

The npm installer package is **`rosetta-repo`**, so the intended invocation is `npx rosetta-repo`. The Rust packages use the same prefix: `rosetta-repo-core`, `rosetta-repo-glossary`, `rosetta-repo-atlas`, `rosetta-repo-cli`, `rosetta-repo-mcp` and `rosetta-repo-lsp`. `cargo install rosetta-repo-cli --locked` installs the `rosetta` binary. The source directories and Rust library import names remain `rosetta-core`, `rosetta_core`, and so on; package names are distinct so they can be published without taking another project's name.

The canonical schema identifiers are `rosetta.glossary/v1` and `rosetta.atlas/v1`. Readers continue to accept `virgil.glossary/v1` and `virgil.atlas/v1` for existing sources. The default model paths are `.rosetta/glossary.json` and `.rosetta/atlas.json`; `.rosetta/rosetta.toml` can select another source directory such as `.virgil`. The generated root documents are `GLOSSARY.md` and `ATLAS.md`.

## Registry checks

These are availability observations, not reservations. Recheck immediately before publishing.

```text
2026-10-26  npm view rosetta version --json
            1.1.0 (unrelated i18n package)
2026-10-26  npm view rosetta-repo version --json
            E404 Not Found - GET https://registry.npmjs.org/rosetta-repo
2026-10-26  cargo search rosetta- --limit 100
            rosetta-core = "0.4.0" (unrelated blockchain package)
            rosetta-cli = "0.4.0" (unrelated blockchain package)
2026-10-26  cargo search rosetta-repo --limit 20
            no matches
2026-10-26  cargo info rosetta-repo-core
            could not find `rosetta-repo-core` in registry
2026-10-26  cargo info rosetta-repo-glossary
            could not find `rosetta-repo-glossary` in registry
2026-10-26  cargo info rosetta-repo-atlas
            could not find `rosetta-repo-atlas` in registry
2026-10-26  cargo info rosetta-repo-cli
            could not find `rosetta-repo-cli` in registry
2026-10-26  cargo info rosetta-repo-mcp
            could not find `rosetta-repo-mcp` in registry
2026-10-26  cargo info rosetta-repo-lsp
            could not find `rosetta-repo-lsp` in registry
```

The public registry entries can be checked at [npm](https://www.npmjs.com/package/rosetta) and [crates.io](https://crates.io/search?q=rosetta-repo). Publication requires the maintainer's registry accounts and is a separate release step.
