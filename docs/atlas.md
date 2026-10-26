# Reading the atlas

For local corrections and agent proposal review, see [Correcting the shared model](curation.md).

For rendering settings and host integration, see [Atlas viewer](viewer.md).

`rosetta init .` creates `.rosetta/atlas.json` from local repository evidence and exports `ATLAS.md`. The JSON model holds curated parts and relationships beside derived workspace structure. `ATLAS.md` is a portable projection of the same bundle used by the local viewer.

```sh
rosetta atlas render --view overview
rosetta atlas render --view overview --format md
rosetta atlas render --view connections --root <part-id> --format md
rosetta atlas show <node-id> --json
rosetta atlas check --json
rosetta atlas check --strict --json
rosetta atlas bundle --out bundle.json
rosetta atlas open
rosetta atlas capabilities
rosetta atlas context --capability <id> --level capability
rosetta atlas context --root <part-id> --depth 2 --max-chars 1500
rosetta atlas impact apps/web/src/main.ts --format md
rosetta atlas changed --base HEAD --format md
rosetta atlas changed --base HEAD --proposal proposal.json --format json
rosetta atlas verify --json
rosetta atlas verify --strict
```

`atlas open` prints a loopback URL and serves the bundled interactive viewer. It reads the local model again for each bundle request, so refresh after `atlas sync` to see new evidence. It works offline, binds only to `127.0.0.1`, and stops with Ctrl-C. The detail panel shows each selected part's responsibility, path, relationships and evidence. Use `--brand brand.json` for optional `primary`, `secondary` and `tertiary` `#RRGGBB` colours; omitted colours use Rosetta's neutral palette. `--port 0` chooses an available port (the default). The viewer build is copied into the Rust crate for `cargo install`; `task viewer:build` refreshes both copies and `task viewer:check` verifies them.

Large overviews open at a readable scale around a connected part. Drag to explore the wider map or choose **Fit whole map** for its full extent. The detail panel's **Focus diagram on connections** action opens that part's incoming and outgoing relationships; on a narrow screen it gives the diagram the panel's width. Select a part to bring its connected paths forward, then use the panel for the full relationship sentence and source. Arrows show flow: a read points from the data source to the reader, while a write points from the writer to its destination. The same convention appears in Mermaid output.

Capabilities are reviewed links from a purpose to glossary terms, responsible parts and implementation paths. `atlas capabilities` lists their stable IDs. `atlas context` selects one capability or part, adds nearby relationships to the requested depth, and prints an agent-sized explanation with a view reference and explicit omissions. `--for <path>` chooses the most specific part covering a path; `--level capability|component|implementation` changes the semantic detail level. `--json` returns the complete selected model slice, including repository and dirty-tree fingerprint, evidence basis, source, extractor and revision. A model-only fingerprint says explicitly when Git coverage is unavailable. A stale reference fails until regenerated with `atlas context --view <ref> --refresh`.

Pass a context reference to `rosetta atlas open --view <ref>` to show the same selected IDs in the browser. The viewer also lists curated capabilities and offers capability, component and implementation levels. Dotted lines in an implementation view link a declared responsible part to an observed source path. The path-exists check does not certify that the code performs the claimed behavior. Manifest dependencies are shown as “depends on”; they do not imply a runtime call or store access.

`atlas impact` selects the most specific part covering a repository-relative path and lists its direct incoming and outgoing typed relationships. `atlas changed --base` compares the current checkout with an exact Git commit. It re-derives structure from both trees, compares glossary and Atlas source separately, and lists changed paths, touched parts, semantic changes, and additional observed structure. A stale committed Atlas cannot make an existing dependency appear newly implemented. It needs a committed `.rosetta/atlas.json` at the baseline; a repo without one should run `rosetta init` and establish a baseline first. For the agent proposal format and review limits, see [Reviewing a proposed change](change-review.md).

`rosetta atlas site --out site` writes a read-only static viewer that opens from a file URL or a static host. See [Static Atlas site](site.md) for files, limits and a Pages workflow.

`atlas verify` evaluates optional `.rosetta/assertions.json` rules against the current local snapshot. It reports pass, violation, or unknown with evidence and repair guidance. Warnings are the adoption default, so results remain visible while the command exits successfully; `--strict` exits nonzero for any violation or unknown. See [Checkable assertions](assertions.md) for the limited evidence coverage.

The default render is Mermaid source with `atlas:` click targets for the local viewer. `--format md` adds the view title, description, focus and legend, then a fenced Mermaid diagram without viewer-only click links. Built views appear when the model has the evidence needed to draw them. `atlas check` validates the source against the repository and reports an edit to generated `ATLAS.md` as a diff; run `atlas sync` and `atlas export` when the model or repository changes. `atlas check --json` emits `rosetta.atlas-check/v1` with `passed`, `strict`, and findings containing a model path and message; it uses the same exit code as text output. `--strict` additionally requires a summary and tier for each human-facing part, plus a glossary term for each surface, service and store when a glossary exists. Family is a required model field in both modes. Strict is an opt-in curation ratchet; a newly derived part can remain visible before its details are written.

SVG export is deferred. It requires a browser-based Mermaid renderer; the installed Rosetta binary keeps this dependency out of the local core. A user who needs SVG can feed the emitted Mermaid to an optional external renderer such as `mmdc`.
