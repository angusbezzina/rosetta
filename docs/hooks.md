# Agent vocabulary feedback

Rosetta checks settled terms and Avoid words in `.rosetta/glossary.json`. Agent integrations provide feedback at different points in the work:

| Agent | During edits | Before commit | Shared context |
| --- | --- | --- | --- |
| Claude Code | `rosetta hooks install claude`: PostToolUse feedback after Edit or Write, plus Stop review | pre-commit hooks | `AGENTS.md` or `CLAUDE.md`, Rosetta skill, optional MCP |
| Cursor | `rosetta hooks install cursor`: `postToolUse` feedback after Write or Edit | pre-commit hooks | `AGENTS.md`, Rosetta skill, optional MCP |
| Codex | Read `AGENTS.md` and run `rosetta glossary lint` during the task | pre-commit hooks | `AGENTS.md`, Rosetta skill, optional MCP |

The hooks give guidance to agents; the Git hooks provide the shared commit check. Install the `rosetta` binary on `PATH` before enabling a hook.

## Claude Code

From an initialized local repository, with `rosetta` on `PATH`:

```sh
rosetta hooks install claude
```

The installer adds two project entries to `.claude/settings.json` and preserves existing settings. Repeating the command does not add duplicates. Claude Code may ask you to trust project hooks when it opens the repository.

- **After Edit or Write:** Rosetta scans the touched TSX, JSX, Markdown, Rust, or Python file for Avoid words in copy. It also checks a touched manifest, configured name source, or CLI declaration for Avoid words in public names. A finding names the preferred term and file line before Claude's next turn.
- **At Stop:** Rosetta scans tracked copy files changed since the branch base and sends remaining findings to Claude once more. It uses the remote default branch when available, then `origin/main`, `main`, `origin/master`, `master`, and finally `HEAD`. Set `ROSETTA_HOOK_BASE` in Claude Code's environment to choose another local ref. No fetch is performed.

Findings are guidance delivered to Claude Code as hook context. They do not undo edits or block a tool call. The Stop hook stops reminding after Claude has already continued once, so it cannot trap the session in a loop. Run `rosetta glossary lint --base <ref>` and `rosetta glossary names --base <ref>` explicitly for a complete report before handoff. The Stop selection includes tracked files; the edit hook also covers a newly written file.

To remove the entries Rosetta installed:

```sh
rosetta hooks uninstall claude
```

Uninstall leaves other project settings and hooks in place. After editing `.claude/settings.json`, restart Claude Code or use `/hooks` to check the active configuration.

## Cursor

From an initialized local repository:

```sh
rosetta hooks install cursor
```

The installer adds one `postToolUse` entry with a `Write|Edit` matcher to `.cursor/hooks.json`. It keeps existing hooks, including Beads hooks, and is safe to repeat. After a matching edit, `rosetta hooks run cursor post-edit` reads Cursor's JSON input, checks the touched file, and returns findings as `additional_context`. Cursor project hooks run from the repository root. [Cursor's hook reference](https://cursor.com/docs/hooks) documents the project file, matcher and response format.

To remove only Rosetta's entry:

```sh
rosetta hooks uninstall cursor
```

## Codex

Add the output of `rosetta agents-snippet` to the repository's `AGENTS.md` and copy the [Rosetta skill](../skills/rosetta/SKILL.md) to `skills/rosetta/SKILL.md` in that repository. [Official OpenAI documentation](https://developers.openai.com/cookbook/examples/codex/iterating-development-workflows-with-codex) confirms Codex reads repository `AGENTS.md` guidance. The snippet points it to the glossary, Atlas, focused context commands and checks. For example:

```text
Read skills/rosetta/SKILL.md when editing concepts, names, or repository structure.
Treat .rosetta/glossary.json and .rosetta/atlas.json as source; render GLOSSARY.md and ATLAS.md.
Use rosetta glossary context and rosetta atlas context for task paths; run their checks before handoff.
```

During a task, run `rosetta glossary lint --files <edited-file>` for immediate vocabulary feedback. Use the pre-commit hooks below for a shared check on staged files. Codex uses the repository instructions and Git hook route; there is no Codex edit hook installed by this command.

## Git commit hooks

The published `.pre-commit-hooks.yaml` supplies `rosetta-glossary-check`, `rosetta-glossary-lint`, `rosetta-atlas-check`, and `rosetta-commit-msg`. Install the Rosetta binary first. In a repository that uses pre-commit 4.4 or later, add a pinned Rosetta release to `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/angusbezzina/rosetta
    rev: v0.1.0 # replace with a published release tag
    hooks:
      - id: rosetta-glossary-check
      - id: rosetta-glossary-lint
      - id: rosetta-atlas-check
      - id: rosetta-commit-msg
```

Run `pre-commit install --hook-type pre-commit --hook-type commit-msg`. The glossary and Atlas checks validate the repository's generated files and links. The lint hook checks staged TSX, JSX, Markdown, Rust and Python copy. The commit-message hook reads Git's message file through `rosetta glossary lint --message-file`; `rosetta glossary lint --stdin` offers the same plain-text check to other tools. Lint findings warn by default and do not stop a commit. Add `args: [--strict]` to either lint hook in the consuming config to make findings fail; repository `lint.strict` also applies.

If the team already uses Lefthook, the equivalent commands can go in `lefthook.yml`:

```yaml
pre-commit:
  commands:
    glossary-check:
      run: rosetta glossary check
    glossary-lint:
      glob: '*.{tsx,jsx,md,rs,py}'
      run: rosetta glossary lint --files {staged_files}
    atlas-check:
      run: rosetta atlas check
commit-msg:
  commands:
    vocabulary:
      run: rosetta glossary lint --message-file {1}
```

With Husky, put `rosetta glossary check`, `rosetta glossary lint --base HEAD`, and `rosetta atlas check` on separate lines in `.husky/pre-commit`; put `rosetta glossary lint --message-file "$1"` in `.husky/commit-msg`. Husky's `--base HEAD` checks tracked working-tree changes against HEAD; for precise staged-only lint, pass your staged file list to `--files` or use the pre-commit definition above.
