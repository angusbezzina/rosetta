# Use the glossary in existing linters

`rosetta glossary emit vale`, `rosetta glossary emit whetstone`, and `rosetta glossary emit json` generate adapters from the **settled** terms in `.rosetta/glossary.json`. Proposed terms do not become rules. Repeat an emit after changing the glossary; the output is deterministic and identical files are left untouched. `rosetta glossary check` checks any adapter that has been staged or committed with Git, including missing or stale Vale rule files.

## Vale

`rosetta glossary emit vale` writes a substitution rule per term with Avoid words to `.vale/styles/Rosetta/Term-<id>.yml`, plus the accepted term names to `.vale/styles/config/vocabularies/Rosetta/accept.txt`. Rules suggest the settled term name for an Avoid word or its plural; a term's `except` phrases are ignored by its rule. The generated style is not automatically enabled. Add these settings to your existing `.vale.ini`, merging them with any current styles:

```ini
StylesPath = .vale/styles
Vocab = Rosetta

[*.md]
BasedOnStyles = Rosetta
```

Then run `vale docs/` or a specific Markdown file. The output was checked with Vale 3.25.1: `workspace` generated a `Group` suggestion while `Slack workspace` did not. Vale evaluates prose with its own parser and scopes; Rosetta's built-in lint remains the check for source copy in code files.

## Whetstone and JSON

`rosetta glossary emit whetstone` writes `.rosetta/emit/whetstone.scm`, byte-for-byte the generated tree-sitter query used by `rule.glossary-terms`. It contains no extra trailing newline so it can be embedded unchanged in a Whetstone rulebook. When `refs.rulebook` names a local rulebook in `.rosetta/rosetta.toml`, `rosetta glossary render` also updates that rule's query in place.

`rosetta glossary emit json` writes `.rosetta/emit/words.json` with schema `rosetta.word-list/v1`; each entry has an id, preferred name, and Avoid words with exception phrases. This is a small adapter for tools that cannot consume the Vale or Whetstone format. The glossary JSON remains the source of truth.
