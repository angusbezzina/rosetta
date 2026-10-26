---
name: rosetta
description: Use a local repository's Rosetta glossary and Atlas to align names, explain responsible parts, review a proposed change, or check work against the shared model. Applies when the repository has .rosetta/; not needed for unrelated edits.
---

# Rosetta in a local repository

Read the task's vocabulary with `rosetta glossary context --for <path>`. For the map, use `rosetta atlas context --for <path> --max-chars 2000`, or choose a capability from `rosetta atlas capabilities` and request it with `--capability <id>`. The context output names the repository snapshot, model revision, selected IDs, evidence and omissions. Give a person the view reference when discussing the same area so they can open it with `rosetta atlas open --view <reference>`.

Treat `.rosetta/glossary.json` and `.rosetta/atlas.json` as the shared source. `GLOSSARY.md` and `ATLAS.md` are generated: use `rosetta glossary render` and `rosetta atlas export` after accepted source changes. `rosetta glossary add` creates a **proposed** term; `rosetta glossary draft --request <dir>` prepares bounded inputs for a draft by the user's agent, and `--from <answer.json>` validates proposals. In a plan, name the proposed term, definition, scope and evidence, and keep its status proposed until the repository's review decision.

Before handing off copy or naming changes, run `rosetta glossary lint --base <base>` and `rosetta glossary names --base <base>`. For each finding, inspect its path, matched word and term ID; use `rosetta glossary show <term-id>` to read the preferred name and scope. Fix the word or record a justified code name or exception in the glossary source. Run `rosetta glossary check` and `rosetta atlas check` after source or model changes. Use `rosetta atlas verify` when the repository declares structural assertions; its **unknown** result is not proof of compliance.

For a proposed implementation, use `rosetta atlas changed --base <base> --proposal <intent.json>` and keep the agent's intent separate from observed file and manifest changes. A dependency or source path alone does not prove runtime behavior. Preserve the evidence and coverage limits in the explanation.
