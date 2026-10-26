# Atlas viewer

`packages/atlas-viewer` is a framework-free TypeScript viewer for an Atlas bundle. `rosetta atlas open` serves its vendored build on loopback; a host can mount `src/index.ts`, supply the bundle and brand variables, and render its own panel from `panelState()` and `onPanelChange()`. The standalone page mounts the local detail panel. The model and focused-view reference select the parts and relationships; pan and zoom are presentation state and do not change that selection.

```sh
cd packages/atlas-viewer
bun test
./node_modules/.bin/tsc --noEmit
bun build.ts --vendor
bun build.ts --check --vendor
```

The tests cover the renderer, bundle reader, node and edge interaction, panel evidence and curation, static read-only mode, keyboard navigation, pan and zoom, narrow layouts and the demo's Mermaid sources. `--vendor` copies the built HTML, CSS, viewer and pinned Pele module into `crates/rosetta-atlas/viewer-dist` so `cargo install` can serve them without Bun.

## Pele rendering settings

`src/pele.ts` pins Pele 1.8.0 and supplies `RENDER_OPTIONS`. Keep the two layout overrides when updating it:

- `autoDirection: false`: Pele otherwise turns a left-to-right diagram top-to-bottom below its 640-pixel direction breakpoint. That silently changes the meaning of a stable map when a side panel opens or the viewport narrows.
- `responsive: false`: Pele otherwise caps its SVG at the container width. A large diagram and its labels shrink together until the text is unreadable. Rosetta keeps the SVG at natural size and controls fitting, panning and zooming itself.

The viewer also sets `zoom: false` because its own camera supports ordinary wheel zoom and pan at the selected point. `limit: 200_000` bounds a source render; Pele's output cap remains in effect. Run the visual checks on a real repository after a renderer update, especially a dense overview and a focused connections view at ordinary and narrow widths. The written source, rendered arrows, readable relationship cards, and detail-panel sentences must agree on relationship kind and evidence. A read arrow shows data flowing toward the reader; the detail panel says which part reads which source.

At widths up to 500 pixels, Connections and focused capability views open as a scrollable **Readable list**. It shows each relationship separately with its arrow, acting-part sentence, and evidence source; focused capability views also list implementation paths and their snapshot existence. The **Diagram** button switches to the same view's SVG. The overview remains a pannable map, with visible capability focus, local Connections, and Details actions. The narrow list keeps long labels readable and leaves the full diagram available for spatial inspection.

Local source previews carry the view or change report's snapshot fingerprint. If the checkout changes before a preview opens, the server rejects the read and the panel offers **Refresh view**; it never silently substitutes current bytes for evidence from an older map. A bundle or context request also stops with an `atlas sync` instruction when saved derived relationships no longer match current repository structure.
