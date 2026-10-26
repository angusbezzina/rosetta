# Drafting a repository glossary

Rosetta reads local files and checks vocabulary, but does not call a model. Your coding agent can propose terms from a bounded request. A proposal is never a settled definition.

```sh
rosetta init .
rosetta glossary draft --request .rosetta/draft
```

Give the agent `.rosetta/draft/prompt.md` and `.rosetta/draft/request.json`. Ask it to write `.rosetta/draft/answer.json`. The request includes only the beginning of the README, document titles, atlas parts, package names, and existing term names. It may omit a concept that needs more context; the agent should use fewer terms instead of guessing.

```sh
rosetta glossary draft --from .rosetta/draft/answer.json
rosetta glossary check
```

The answer has `purpose`, `sections` (`id`, `title`), and `terms` (`name`, `section`, `definition`). Rosetta validates each proposed term, reports rejected terms with reasons, and leaves existing terms untouched. Joined terms have `status: "proposed"`. Review each definition and its evidence with a person before changing its status to `settled`. Commit the reviewed `.rosetta/glossary.json` and generated `GLOSSARY.md` together.

This works with Claude Code, Codex, Cursor, or another agent that can read two local files and write one JSON answer. The installed Rosetta core needs no key or network connection for these commands.
