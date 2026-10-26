# Reviewing a proposed change

An agent can describe intended relationships and model edits with stable Atlas IDs. Save the JSON to a local file and pass it to `rosetta atlas changed --base <commit> --proposal <file> --format md`, or import it through **Review an agent proposal (JSON file)** in `rosetta atlas open`. The local viewer marks touched parts and lists intended, observed, and additional structural changes. The file is a review overlay; importing it does not edit the model or repository.

```json
{
  "schema": "rosetta.impact-proposal/v1",
  "base_commit": "<full Git commit ID>",
  "base_model_revision": "<model revision from atlas changed --base>",
  "intents": [
    {
      "kind": "relationship",
      "from": "apps-web",
      "to": "packages-helper",
      "relation": "depends",
      "present": true,
      "rationale": "The web app should use the shared helper."
    },
    {
      "kind": "responsibility",
      "part": "door",
      "value": "Answers one request at a time.",
      "rationale": "Correct the shared description."
    }
  ]
}
```

`relationship` supports a typed edge and `present: true|false`. `capability_part`, `responsibility`, `term_definition`, and `claim` cover other intentions. A supported structural edge is **implemented** only when the extractor observes the requested change relative to the baseline. A source model or glossary value matching an intention is **declared in model**; that result does not prove the code behaves as described or that a person reviewed the edit. Other results are **still proposed** or **unknown**. Additional derived changes appear separately. Calls, data access, latency, and other behavioral claims remain unknown without a supporting extractor.

The baseline commit and source-model revision must still match the proposal. Regenerate a stale proposal against the selected baseline. To correct the shared model, select a part in the viewer, preview a correction, and explicitly apply it using [Correcting the shared model](curation.md). To correct an agent's intent overlay, edit its JSON file and import it again. The local viewer does not apply an impact proposal as a curation proposal.
