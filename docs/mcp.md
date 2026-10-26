# Ask Rosetta from an agent

Rosetta's MCP server reads one **local** repository. It exposes the same glossary and focused Atlas view used by the CLI and viewer. The server reads source and live repository state for each request, so a view reference becomes stale after a model or code change. It never writes the model or accepts a proposed change.

Build the binary from this checkout:

```sh
cargo build -p rosetta-repo-cli --locked
```

Use absolute paths to the resulting `target/debug/rosetta` and the repository that contains `.rosetta/glossary.json` and `.rosetta/atlas.json`. Confirm the command starts with `rosetta mcp --repo /absolute/path/to/repo`. It speaks JSON-RPC over stdio, so it will wait quietly for a client.

## Connect an agent

**Claude Code** (run in the target project):

```sh
claude mcp add --scope project rosetta -- /absolute/path/to/rosetta mcp --repo /absolute/path/to/repo
claude mcp list
```

The project scope records the command in the project's `.mcp.json` and Claude Code may ask users to approve project servers. See [Claude Code's MCP setup](https://code.claude.com/docs/en/mcp).

**Codex CLI and IDE:**

```sh
codex mcp add rosetta -- /absolute/path/to/rosetta mcp --repo /absolute/path/to/repo
codex mcp list
```

This registers a server in Codex's shared configuration. Use a distinct server name when connecting more than one repository. See [Codex MCP documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

**Cursor:** Add this to the target project's `.cursor/mcp.json` (merge it with any existing `mcpServers` entries):

```json
{
  "mcpServers": {
    "rosetta": {
      "command": "/absolute/path/to/rosetta",
      "args": ["mcp", "--repo", "/absolute/path/to/repo"]
    }
  }
}
```

Open Cursor's MCP tools list after reloading the project. See [Cursor's MCP documentation](https://docs.cursor.com/context/model-context-protocol).

## Use the shared map

Ask the agent to find a part with `list-parts`, call `open-view` with a capability ID, part ID or repository path, then explain using stable part and term IDs and the returned `reference`. The user can reopen that reference in the viewer or CLI. `read-view` reopens it in MCP; if the snapshot changed, it reports staleness. `terms-for-paths` accepts the same reference to select the same vocabulary. `incomers` and `outgoers` preserve the model's directed edges and evidence. A `reads` edge in the Virgil pilot points from the reader to its store, so `incomers` on the store identifies modeled readers.

Every tool accepts `max_chars` (default 8,000; range 256–100,000) and returns structured JSON. If a response needs less space, Rosetta removes whole optional items and reports `omitted_for_budget`; focused views also report the selection's `omissions`. Increase the budget or reopen a narrower view if necessary. The `GLOSSARY.md` and `ATLAS.md` resources provide the full generated pages.

`propose-change` accepts a `rosetta.impact-proposal/v1` object grounded in an existing view reference. It checks the current HEAD, model revision and referenced IDs, then marks the result `proposed_unverified`. It does not edit source files or turn a proposal into an observed relationship. Use the viewer's **Review an agent proposal** flow for a human review and the CLI's `atlas changed` for evidence against a baseline.
