import type { AtlasHandle } from "./viewer.ts";
import { evidenceDetails, mountCuration, type LocalSession } from "./curation.ts";
import { relationVerbs } from "./relationship.ts";

/** Local detail panel. Hosts can omit this and render their own. */
export function mountPanel(atlas: AtlasHandle, session?: LocalSession): () => void {
  const doc = atlas.panel.ownerDocument;
  const element = <K extends keyof HTMLElementTagNameMap>(tag: K, text?: string): HTMLElementTagNameMap[K] => {
    const node = doc.createElement(tag);
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const section = (title: string) => {
    const root = element("section");
    root.append(element("h3", title));
    return root;
  };
  const paragraph = (label: string, value: string) => {
    const row = element("p");
    row.append(element("strong", `${label}: `), doc.createTextNode(value));
    return row;
  };
  const render = () => {
    const { bundle, trail } = atlas.panelState();
    const selected = trail.at(-1);
    const node = bundle.nodes.find((candidate) => candidate.id === selected);
    const root = element("div");
    root.className = "rosetta-detail";
    const header = element("header");
    const close = element("button", "Close");
    close.type = "button";
    close.setAttribute("aria-label", "Close detail panel");
    close.addEventListener("click", () => atlas.setPanelOpen(false));
    header.append(element("h2", node?.label ?? "Details"), close);
    root.append(header);
    if (session?.can_undo) mountCuration(root, bundle, node, session, doc, atlas);
    if (!node) {
      root.append(element("p", "Select a part to inspect its meaning, code and connections."));
      atlas.panel.replaceChildren(root);
      return;
    }
    const connections = bundle.edges.filter((edge) => edge.from === node.id || edge.to === node.id);
    const connectionsView = bundle.views.find((view) => view.kind === "connections" && node.id in view.sources);
    if (connectionsView && connections.length > 0) {
      const focus = element("button", `Focus diagram on ${connections.length} connections`);
      focus.type = "button";
      focus.addEventListener("click", () => {
        atlas.open(connectionsView.id, node.id);
        // Give the focused diagram the width it needs on a narrow screen.
        if ((doc.defaultView?.innerWidth ?? 1024) <= 1000) atlas.setPanelOpen(false);
      });
      root.append(focus);
    }
    root.append(paragraph("Kind", `${node.family} · ${node.kind}`));
    if (node.summary) root.append(paragraph("Responsibility", node.summary));
    if (node.path) root.append(paragraph("Path", node.path));
    if (node.tier) root.append(paragraph("Tier", node.tier));
    if (node.updated) root.append(paragraph("Last change", `${node.updated.at} · ${node.updated.subject}`));

    const term = bundle.terms.find((candidate) => candidate.id === node.refs.term);
    if (term) {
      const meaning = section("Shared vocabulary");
      meaning.append(paragraph(term.status === "proposed" ? "Proposed term" : "Term", term.name));
      meaning.append(element("p", term.definition));
      root.append(meaning);
    }

    if (bundle.focused_view_id && bundle.terms.length > 0) {
      const vocabulary = element("details");
      vocabulary.className = "rosetta-focused-terms";
      vocabulary.append(element("summary", `Vocabulary in this view (${bundle.terms.length})`));
      for (const entry of bundle.terms) {
        const row = element("p");
        row.append(element("strong", `${entry.name}${entry.contexts?.length ? ` (${entry.contexts.join(", ")})` : ""} · ${entry.status}: `), doc.createTextNode(entry.definition));
        vocabulary.append(row);
      }
      root.append(vocabulary);
    }

    const capabilities = (bundle.capabilities ?? []).filter((capability) =>
      capability.parts.includes(node.id) || capability.implementation.some((target) => target.part === node.id)
    );
    if (capabilities.length > 0) {
      const list = section(`Capabilities (${capabilities.length})`);
      for (const capability of capabilities) {
        const entry = element("div");
        entry.append(paragraph(capability.status === "proposed" ? "Proposed" : "Capability", capability.name));
        entry.append(element("p", capability.purpose));
        const terms = capability.terms.map((id) => bundle.terms.find((candidate) => candidate.id === id)?.name ?? id);
        if (terms.length > 0) entry.append(paragraph("Terms", terms.join(", ")));
        const targets = capability.implementation.filter((target) => target.part === node.id);
        for (const target of targets) {
          const exists = bundle.target_exists?.[target.path];
          entry.append(paragraph("Implementation link", `${target.path}${target.symbol ? `#${target.symbol}` : ""} · ${exists === false ? "path missing" : exists === true ? "path exists" : "not checked"}`));
          if (session && bundle.snapshot && exists === true) entry.append(evidenceDetails(doc, session, target.path, bundle.snapshot.fingerprint));
        }
        list.append(entry);
      }
      root.append(list);
    }

    const children = bundle.nodes.filter((candidate) => candidate.parent === node.id);
    if (children.length > 0) {
      const list = section(`Inside (${children.length})`);
      const items = element("ul");
      for (const child of children) {
        const item = element("li");
        const button = element("button", `${child.label} · ${child.kind}`);
        button.type = "button";
        button.addEventListener("click", () => atlas.drill(child.id));
        item.append(button);
        items.append(item);
      }
      list.append(items);
      root.append(list);
    }

    if (connections.length > 0) {
      const list = section(`Connections (${connections.length})`);
      list.append(element("p", "Each sentence names the acting part first. Diagram arrows show flow; a read arrow points from the source to the reader."));
      for (const [title, actingPart, edges] of [
        ["What other parts do", false, connections.filter((edge) => edge.to === node.id)],
        ["What this part does", true, connections.filter((edge) => edge.from === node.id)],
      ] as const) {
        if (edges.length === 0) continue;
        list.append(element("h4", `${title} (${edges.length})`));
        const items = element("ul");
        for (const edge of edges) {
          const otherId = actingPart ? edge.to : edge.from;
          const other = bundle.nodes.find((candidate) => candidate.id === otherId);
          const from = actingPart ? node.label : other?.label ?? otherId;
          const to = actingPart ? other?.label ?? otherId : node.label;
          const item = element("li");
          const button = element("button", `${from} ${relationVerbs[edge.kind]} ${to}${edge.label ? ` · ${edge.label}` : ""}`);
          button.type = "button";
          button.addEventListener("click", () => atlas.drill(otherId));
          item.append(button);
          const evidence = edge.evidence;
          const basis = evidence?.basis.replaceAll("_", " ") ?? (edge.derived ? "derived source" : "declared intent");
          const source = evidence?.source ?? edge.derived?.source ?? "atlas model";
          const stale = evidence?.revision && bundle.snapshot && evidence.revision !== bundle.snapshot.fingerprint;
          item.append(element("small", `${basis} · ${source}${stale ? " · evidence from an earlier revision" : ""}`));
          if (session && bundle.snapshot && source !== "atlas model" && !stale) item.append(evidenceDetails(doc, session, source, bundle.snapshot.fingerprint));
          items.append(item);
        }
        list.append(items);
      }
      root.append(list);
    }

    const refs = [
      ...node.refs.rules.map((id) => ["Rule", id] as const),
      ...node.refs.principles.map((id) => ["Principle", id] as const),
      ...node.refs.docs.map((id) => ["Document", id] as const),
    ];
    if (refs.length > 0) {
      const evidence = section("Evidence and guidance");
      for (const [kind, id] of refs) {
        const rule = bundle.rules.find((candidate) => candidate.id === id);
        evidence.append(paragraph(kind, rule?.title ?? id));
      }
      root.append(evidence);
    }
    if (session && !session.can_undo) mountCuration(root, bundle, node, session, doc, atlas);
    atlas.panel.replaceChildren(root);
  };
  render();
  return atlas.onPanelChange(render);
}
