# Rosetta

One binary that keeps people and agents on the same words and the same map.

- `rosetta glossary`: a repository's terms, defined once, rendered to `GLOSSARY.md`, enforced where copy, names and agent output are written.
- `rosetta atlas`: a repository's parts and how they connect, derived from its workspaces, curated by people, rendered to `ATLAS.md` and Mermaid views, failing CI when it lies.

Independent of Virgil; works with it. Planning lives in beads (`bd ready`).

The first local-repository journey has a [reproducible Virgil pilot](examples/virgil-pilot/README.md), including a structural change review and an explicit human evaluation protocol.

Agents can use the [Rosetta skill](skills/rosetta/SKILL.md). Run `rosetta agents-snippet` for a three-line pointer to paste into a repository's `AGENTS.md` or `CLAUDE.md`.

Connect Claude Code, Codex or Cursor through the [local MCP server](docs/mcp.md) to ask about terms, parts, evidence and reopenable focused views.

Claude Code and Cursor users can [install vocabulary feedback hooks](docs/hooks.md); Codex can use the same repository instructions and Git checks.

Documentation teams can [emit Vale, Whetstone, or JSON adapters](docs/glossary-emit.md) from the same settled vocabulary.

Pull requests can use the [Rosetta composite action](docs/action.md) for vocabulary annotations and optional strict checks after the first release.

The [offline boundary](docs/offline.md) covers local analysis and the viewer after installation.
