# Repository configuration

Rosetta works without configuration. If needed, add `.rosetta/rosetta.toml`; `rosetta config show --json` prints the effective values. Unknown keys fail with the source line. Paths are relative to the selected local repository, and configured JSON providers cannot follow a symlink outside it. There is no account or model setting in the local core.

```toml
[source]
directory = ".rosetta"

[refs]
# Set only when a local Whetstone rulebook supplies rule and principle refs.
# rulebook = "whetstone/rulebook.json"

[[refs.providers]]
kind = "rule"
path = "rules/catalog.json"
pointer = "/rules"

[[name_sources]]
glob = "schemas/*.json"
pointer = "/collections"

[records]
collections_key = "x-rosetta-collections"
scope_key = "x-rosetta-scope"
ref_to_key = "x-rosetta-ref-to"

[derivation]
iac = true
drivers = true

[[path_scopes]]
context = "frontend"
paths = ["apps/web/**"]

[tracker]
command = []
# closed_pointer = "/result/task/state"

[lint]
strict = false
formats = []
include = []
exclude = []

[names]
# Omit a field to use common CLI paths; [] disables that scanner.
rust_cli = ["crates/my-cli/src/cli.rs"]
typescript_cli = ["tools/cli.ts"]
typescript_ids = ["packages/schema/src/**/*.ts"]
```

| Key | Default | Meaning |
| --- | --- | --- |
| `source.directory` | `.rosetta` | Where `atlas.json` and `glossary.json` live. The config file itself stays in `.rosetta/rosetta.toml` so Rosetta can find the override. |
| `refs.rulebook` | unset | Repository-relative path to a local Whetstone rulebook. It supplies rules, principles and the optional glossary enforcer query when configured. A repository without one can still validate document refs against local files. |
| `refs.providers` | `[]` | Additional JSON files containing `rule`, `principle` or `document` IDs. Each entry has `kind`, repo-relative `path` and JSON `pointer`. Objects contribute keys; arrays contribute strings or objects with `id`/`name`. A missing file or pointer is an error. |
| `name_sources` | `[]` | Repo-relative JSON globs with a `pointer` to code names such as record collections. Without one, glossary code-name validation uses workspace Cargo and package.json names only. A glob with no matches is allowed; a matched file with a bad pointer is an error. |
| `records.collections_key` | `x-rosetta-collections` | Top-level JSON Schema property containing the record collection map. The Atlas model selects the file with `records.file`. |
| `records.scope_key` | `x-rosetta-scope` | Collection JSON Schema property naming its scope. |
| `records.ref_to_key` | `x-rosetta-ref-to` | Field JSON Schema property naming referenced collections. For all three Records keys, the configured property takes precedence; `x-rosetta-*` and legacy `x-virgil-*` properties are accepted as aliases. |
| `derivation.iac` | `true` | Include supported infrastructure declarations in Atlas derivation. |
| `derivation.drivers` | `true` | Include supported SDK and driver dependencies as dependency evidence. A dependency does not establish a runtime call, read or write. |
| `path_scopes` | `[]` | Named contexts for repository-relative path globs. A glossary term can set `contexts` to one or more names. `glossary context --for <path>` selects matching terms; a term without `contexts` is repository-wide. Two meanings may share a name only when their contexts do not overlap. |
| `tracker.command` | `[]` | Declared command and arguments for closed-epic checks, reserved for an explicit tracker adapter. Currently parsed and shown, but not run; `glossary check` never launches a tracker from `PATH`. |
| `tracker.closed_pointer` | unset | JSON pointer into the command response, reserved with `tracker.command`. |
| `lint.strict` | `false` | Make `glossary lint` findings fail the command by default. `--strict` also enables this per invocation. |
| `lint.formats` | `[]` | Reserved for future lint integrations; the CLI currently accepts `--format text`, `json`, or `sarif`. |
| `lint.include` | `[]` | Repository-relative globs selected by a full or `--base` copy scan. Empty means all supported tracked files. |
| `lint.exclude` | `[]` | Repository-relative globs omitted from a full or `--base` copy scan. Explicit `--files` bypasses both filters. |
| `names.rust_cli` | common `**/src/cli.rs`, `**/src/main.rs` paths | Rust files whose reachable clap Parser, Subcommand and positional ValueEnum declarations expose command words. Set `[]` to disable. A library CLI file with no local Parser scans its Subcommand enums, since its Parser can live in another crate. |
| `names.typescript_cli` | common CLI/script paths | TypeScript files with commander or yargs `.command(...)` declarations. Set `[]` to disable. |
| `names.typescript_ids` | `[]` | TypeScript source globs where `id: "…"` and `*Collection("…")` declare identifiers. Keep these scoped to the schema source files for the repository. |

`name_sources` JSON pointers also feed `glossary names`: each string, object key or object `id`/`name` at the configured pointer is a naming boundary. `glossary names --base <ref>` scans only tracked files changed since the merge base; `--files` scans specified paths. It warns by default and fails with `--strict` or `lint.strict = true`. Text, JSON and SARIF output carry the term to use. Names recorded exactly in any term's `code` list and phrases listed under an Avoid word's `except` are exempt.

The source directory override is useful while moving a legacy `.virgil/atlas.json` and `.virgil/glossary.json` into Rosetta. An explicit `rosetta glossary --source <repo-relative-file>` takes precedence for that command. `atlas check` reports invalid model links and a difference between generated and committed `ATLAS.md`.
