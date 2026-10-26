# Vocabulary in context

`rosetta glossary context --for apps/web/src/index.ts --max-chars 2000` gives a bounded list of term IDs, names, definitions and Avoid words for a local path. Repeat `--for` for multiple files. `--format json` returns the complete selection. Without a path, all terms are selected.

Configure a path context in `.rosetta/rosetta.toml`:

```toml
[[path_scopes]]
context = "web"
paths = ["apps/web/**"]
```

Then add `"contexts": ["web"]` to a glossary term. Terms with no `contexts` apply throughout the repository. A name can have separate definitions in disjoint contexts; use `glossary show <term.id>` to select one precisely.

For selected paths covered by an Atlas, the output includes the responsible parts, a snapshot fingerprint, and an Atlas view reference. Without paths, the output is the full glossary and has no focused view reference. `rosetta glossary context --view <reference>` reopens a focused selection and rejects it after a repository or model change. `rosetta atlas open --view <reference>` opens the same selection for a person. The text budget removes Avoid lists first, then definitions not mentioned in the selected files, then remaining detail and finally terms; the footer counts every omission. JSON retains all selected terms.
