# Local Virgil glossary and Atlas pilot

This is a reproducible structural exercise for the first Rosetta journey. It uses Virgil revision `aece0f5ee1f1f1367b6805a2d45457868533b0d5` from a **local** repository, clones it into a separate checkout, and seeds an explicitly proposed glossary/Atlas capability. The original Virgil checkout is only read. The setup makes a baseline commit in the isolated clone so `atlas changed` can compare source and observed structure. Nothing is pushed.

## Prepare

From the Rosetta repository, build the local binary, then run:

```sh
cargo build -p rosetta-repo-cli
bun examples/virgil-pilot/prepare.ts /path/to/local/virgil
```

The command prints `checkout`, `baseline`, `intent`, and the measured offline `init_seconds`. It refuses a missing source revision or an existing destination path. For another binary, set `ROSETTA_BIN` to its absolute path. The seeded responsibility of `packages-virgil-atlas-viewer` falsely says opening a view rewrites glossary definitions. The seeded capability remains `proposed`; its terms come from the selected Virgil glossary. The generated intent file lives beside the clone so reading it does not itself change the repository snapshot.

The clone also receives `.rosetta/rosetta.toml` with Virgil's existing product-copy file scope. Run `rosetta glossary lint` there for a warning-only scan, or add `--strict` to make findings fail. The scope is explicit so technical docs and internal source strings do not silently become product-copy violations.

## Moderated journey

The participant should be unfamiliar with this area; include at least one product-oriented participant. Give them the isolated checkout and Rosetta CLI, and record any facilitator prompts. Let them use their usual coding agent with the bounded `atlas context` and `glossary context` output. Do not supply the answers in advance.

1. Ask where the glossary/Atlas capability lives, what **Atlas** and **Rulebook** mean here, and which parts own the work. Start with `rosetta atlas open` at the repository overview. They can select **Explain the repository with shared vocabulary** and move through Capability, Component, and Implementation. The focused bar exposes the shared view reference, preserves the selected part in the URL, and offers **Details & evidence**. At narrow widths, the focused view opens as a readable relationship and implementation list; **Diagram** shows the same view spatially. Returning to the full Atlas opens that part's Connections when it is absent from the broad overview. The same selection is available to an agent with `rosetta atlas context --capability glossary-atlas --level implementation --max-chars 2000`. Have them open at least one source file from a connection or implementation link.
2. Give the agent and participant the printed intent JSON. Ask what each proposed relationship or statement would change, and what the map can actually verify. Import the JSON through **Review an agent proposal** in the local panel, or use `rosetta atlas changed --base <baseline> --proposal <intent> --format md`. The Markdown report names each intent and its assessment.
3. Ask the participant to find and correct the seeded false **Atlas viewer** package responsibility through **Correct the shared model**. They should preview the before/after diff, apply it, and see the focused view refresh to the new snapshot. This curation records a model statement; it does not prove behavior. `rosetta atlas check` and `rosetta glossary check` should pass afterward.
4. After they have reviewed the intent, run `bun examples/virgil-pilot/apply-structural-probe.ts <checkout> <intent>` from Rosetta. This intentionally adds a schema dependency and an unplanned observability dependency to the isolated viewer manifest, then syncs the derived map. The probe changes manifest structure only; it does not claim the viewer uses the schema at runtime. A bundle or context request made after editing a manifest but before syncing reports stale derivation instead of claiming old edges are current evidence. Ask the participant to distinguish the implemented **depends** edge, the unimplemented presence dependency, the unexpected observability dependency, the model-only corrected responsibility, and the unknown behavioral claim. Have them open the manifest evidence in the panel. If they leave a view open while code changes, its source preview asks them to refresh before reading newer bytes.
5. Run `rosetta atlas verify` in the clone and ask what its pass, violation, and unknown results mean. The observability boundary should be a violation with the manifest path and repair. `rosetta atlas verify --strict` should exit nonzero; the default command reports findings without failing. For the separate broken-link and removed-required-edge checks, temporarily change the capability target path and remove the existing viewer-to-views dependency in the isolated clone, run `verify`, then restore those two files. The fixture suite covers the same cases without modifying the pilot.

The proposal file is an overlay. Editing it and importing again corrects an agent's *proposal*; using the panel corrects the shared *model*. A source-model match is reported as `declared_in_model`, and a runtime claim without supported evidence remains `unknown`.

## Record each participant

| Step | Correct without help? | Time | Assistance or disagreement | Corrections | Evidence opened |
| --- | --- | --- | --- | --- | --- |
| Find and explain capability, terms, owners |  |  |  |  |  |
| Navigate to implementation |  |  |  |  |  |
| Explain proposal before code change |  |  |  |  |  |
| Correct seeded responsibility |  |  |  |  |  |
| Compare proposed and observed structure |  |  |  |  |  |
| Explain assertion results |  |  |  |  |  |

Also record the checkout revision, baseline commit, Rosetta revision, viewport, participant's prior familiarity, whether they used an agent, and any places they inferred behavior from a dependency. File concrete failures in Beads under `rosetta-qjl.14`. Keep the milestone open until an unfamiliar participant completes the continuous journey; an automated rehearsal is only a technical check.

## Automated rehearsal, 2026-10-25

An isolated local clone of the named Virgil revision completed `rosetta init` offline in 5.02 seconds and produced 233 evidenced nodes and 471 relationships with an honest empty glossary starter. The checked-in `prepare.ts` repeated setup in 4.38 seconds for `init` on the same machine. At a 2,000-character context budget, the focused agent output included both owning parts, all three implementation links, and the relevant glossary terms with explicit omissions. The interactive view loaded a 6.6 MB bundle in headless Chrome and showed the glossary/Atlas focus at 1440×900 and 900×800. At the narrow width, the diagram retains readable labels and offers **Fit whole map**.

Through the browser panel, the seeded false responsibility was previewed and corrected without JSON editing; the view regenerated after the model revision changed. A proposal import showed one structurally implemented dependency, one still proposed dependency, one unexpected dependency, a model-only responsibility change, and an unknown behavioral claim. Opening the source preview displayed the exact viewer `package.json`. The assertion report detected the unexpected forbidden dependency. Separately planted missing implementation and removed required dependency produced violations with path, part, term, coverage, and repair; the unsupported `calls` assertion remained unknown. `atlas check` and `glossary check` passed after restoring the two temporary violations.

These are automated observations, not participant effectiveness results. No unfamiliar person has yet completed the moderated journey. The local clone and browser screenshots are kept outside this repository because they contain Virgil source details.

## Vocabulary rehearsal, 2026-10-26

The scoped offline linter scanned 532 tracked Virgil product-copy files in 0.7 seconds. It reported 11 occurrences: eight visible uses of **recap** where the settled term is **Summary**, plus three uses of **workspace** in the known Linear integration review area. Those three need a human wording decision because some refer to Linear's own workspace. No finding outside that known area was a parser or code-identifier false positive. `--strict` turns the same findings into a failing check; default warnings leave the command successful. This does not mean the Virgil product copy has been edited.

The pilot configuration also scopes naming checks to Virgil's clap CLI and schema source. `rosetta glossary names` and the existing `whetstone/checks/glossary-names.ts` checker each found the same one issue in the pinned tree: the `beads_import` collection identifier uses the settled Avoid word **bead**. The pilot does not rename Virgil's collection; a schema owner must decide whether to rename it or record a justified code-name exception. A separate fixture proves that a newly named `workspace-core` crate is reported in changed-only mode.

The Atlas completeness ratchet is optional. `rosetta atlas check --json` passes on the isolated pilot snapshot; `rosetta atlas check --strict --json` lists 16 specific curation gaps: seven missing glossary links, five missing summaries and four missing tiers. These are model fields to review, not newly inferred repository facts. The normal check remains green while the team curates them.

## Diagram review, 2026-10-26

In the initial 1440×900 overview, fitting the whole 3,642-pixel-wide Virgil map made labels too small to read. The revised overview opens at readable scale near a connected part, highlights that part's incoming and outgoing paths, and offers **Drag to explore** and **Fit whole map**. At 900×800 the map remains pannable; the selected part and nearby labels stay readable. The new **Focus diagram on 15 connections** action opens Daemon's grouped local view, where all four incoming and eleven outgoing relationships retain their distinct labels and evidence. On a narrow screen that action closes the side panel so the diagram gets more width; the panel can be reopened for source inspection. A second 900×800 visual check exposed clipped outgoing nodes in this focused view; the viewer now fits moderate-sized Connections views on first open. The repeat screenshot shows both sides and all 15 connections within the frame, with readable labels. The legend explains why a read arrow points from data source to reader, and a write arrow points to its destination. The full overview still requires panning for distant parts, which is deliberate at a readable scale.

A further first-visit review at 900×800 showed that the default open detail panel left too little width for the overview. The viewer now starts with the panel closed when its host is at most 1,000 pixels wide, while remembering an explicit choice to reopen it. The fresh-visit screenshot shows more of the left-to-right map at a readable scale; clicking Daemon opens the focused 15-connection view without losing the ability to reopen evidence. This is a navigation improvement, not a claim that the entire 3,642-pixel overview fits at once.

Screenshots from the local review are outside this repository: `/tmp/rosetta-virgil-overview-1440.png` (initial fitted map), `/tmp/rosetta-virgil-overview-legend.png` and `/tmp/rosetta-virgil-overview-legend-900.png` (readable overview with focus action), `/tmp/rosetta-daemon-connections-1440.png` and `/tmp/rosetta-daemon-connections-900-nopanel.png` (focused connections before the narrow framing fix), and `/tmp/rosetta-daemon-connections-900-refit.png` (the focused view after it). This is a technical visual review by the implementer; the unfamiliar participant's navigation and interpretation remain unmeasured.

The latest screenshots are `/tmp/rosetta-overview-900-first-visit.png` (new narrow first visit) and `/tmp/rosetta-overview-900-panel-default.png` (the focused Connections view reached by clicking Daemon). They were captured from the same local Virgil pilot checkout after rebuilding the vendored viewer.

The 390×844 technical review used a fresh isolated checkout of the pinned Virgil revision. `/tmp/rosetta-overview-compact-390.png` shows the overview with its compact capability and Connections controls; `/tmp/rosetta-daemon-list-390.png` shows all 15 Daemon relationships as individual readable cards; `/tmp/rosetta-focus-list-390.png` and `/tmp/rosetta-implementation-list-390.png` show the focused component and implementation levels with paths and evidence. The 1440-pixel overview and Daemon Connections remain diagram-first in `/tmp/rosetta-overview-current-1440.png` and `/tmp/rosetta-daemon-focus-1440.png`. The list follows the same read-arrow rule as the diagram, while its sentence names the part doing the reading. These screenshots document layout and semantics only; the unfamiliar-user acceptance remains open.
