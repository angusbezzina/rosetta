# Correcting the shared model

Run `rosetta atlas open` in a local checkout, select a part, and choose **Correct the shared model** in its detail panel. Choose a field, explain the correction, preview the exact before and after values, then explicitly apply it. Rosetta validates the new source before writing, updates the generated `GLOSSARY.md` or `ATLAS.md`, and offers a one-step undo. If a source or generated file changes after apply, undo refuses to overwrite that edit.

An agent can write a proposal as JSON and hand it to a person to review with **Review an agent proposal (JSON file)** in the same panel. The agent does not need a model key or a Rosetta account. A minimal proposal is:

```json
{
  "schema": "rosetta.curation/v1",
  "base_model_revision": "<model_revision from atlas bundle or context>",
  "rationale": "The Door answers requests, but it does not stream responses.",
  "changes": [
    { "kind": "set_node_summary", "id": "door", "value": "Answers requests and returns one response." }
  ]
}
```

Supported changes are `set_node_label`, `set_node_summary`, `set_node_term`, `set_term_name`, `set_term_definition`, `set_capability_name`, `set_capability_purpose`, and `set_capability_part`. They reference stable IDs already in the source. A term link uses `{"kind":"set_node_term","id":"door","term":"term.door"}`; use `null` to clear it. A capability link uses `{"kind":"set_capability_part","capability":"explain-a-request","part":"door","present":true}`. Edits to derived part labels and responsibilities are explicit curation overrides retained by Atlas sync.

The `base_model_revision` checks that the source has not changed since the proposal was prepared. A stale proposal must be reviewed again against the current model. Preview never writes. Apply is available only in the loopback viewer, with an origin check and session token; a static export keeps the inspection panel read-only. Proposed glossary terms remain proposed after an edit to their name or definition.
