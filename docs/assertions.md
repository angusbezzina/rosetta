# Checkable assertions

Add `.rosetta/assertions.json` after reviewing the relevant Atlas part and glossary term IDs. Run `rosetta atlas verify` for human output or `rosetta atlas verify --json` for an agent or CI. Each result names the local snapshot, rule, related part and term, source evidence, coverage limit, and a repair suggestion.

```json
{
  "schema": "rosetta.assertions/v1",
  "rules": [
    {
      "kind": "implementation_exists",
      "id": "explain-target",
      "capability": "explain-a-request",
      "part": "door",
      "term": "term.door"
    },
    {
      "kind": "dependencies",
      "id": "web-boundary",
      "part": "web",
      "allowed": ["packages-ui"],
      "forbidden": ["packages-helper"]
    },
    {
      "kind": "required_relationship",
      "id": "web-uses-ui",
      "from": "web",
      "to": "packages-ui",
      "relation": "depends"
    }
  ]
}
```

`implementation_exists` checks the implementation paths already declared on a capability, optionally limited to one part. It checks path existence inside the repository; symbols and code behavior are outside coverage. `dependencies` checks discovered internal Cargo and Node workspace dependencies. Omit `allowed` to permit any internal dependency except those in `forbidden`; supply an empty `allowed` list to permit none. `required_relationship` can prove the presence or absence of an internal manifest `depends` edge when both ends are discovered workspace members. Calls, reads, writes and other behavioral relationships return **unknown** if a rule asks for their absence. Unknown also covers unsupported member paths or unresolved glossary links.

Rules are evaluated against the current local tree, including uncommitted edits. The snapshot fingerprint and model revision change when the rules change. A missing assertions file yields no checks. The default command reports violations and unknown checks without failing, so teams can adopt rules gradually. Use `--strict` when both violations and unknown coverage should fail CI. The rule set makes structural promises only; it does not certify runtime behavior or whether a responsibility statement is true.
