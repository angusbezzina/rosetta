// The entry dist/atlas-viewer.js is built from: the viewer, its detail panel
// and the boot the CLI's local page runs.
import { type AtlasBrand, type AtlasTheme, resolveBrand } from "./brand.ts";
import { mountPanel } from "./panel.ts";
import type { LocalSession } from "./curation.ts";
import { readBundle } from "./read.ts";
import { type AtlasHandle, mount } from "./viewer.ts";
import type { AtlasBundle, FocusedView } from "./types.ts";

export * from "./index.ts";

declare global {
  interface Window {
    __ATLAS__?: { bundle: unknown; brand?: Partial<AtlasBrand>; staticSite?: boolean };
  }
}

/**
 * Mount the atlas the page was served with: `window.__ATLAS__` when the page
 * inlines it, else `./bundle.json` and `./brand.json` beside the page (the
 * brand is optional; resolveBrand fills what it lacks with neutral defaults), with
 * the detail panel drawn beside it. The theme follows the system; the page's
 * root carries `dark` while it is dark, which the panel styles use.
 *
 * A bundle the page cannot have (the server answered with a failure, or what
 * it answered is not a bundle) is said in the page, in the server's own words
 * when it gave some, and boot answers null: a blank page explains nothing.
 */
export async function boot(el: HTMLElement): Promise<AtlasHandle | null> {
  const given = window.__ATLAS__ ?? (await fetchServed());
  if ("failure" in given) return unavailable(el, given.failure);
  let bundle: ReturnType<typeof readBundle>;
  try {
    bundle = readBundle(given.bundle);
  } catch (error) {
    return unavailable(
      el,
      error instanceof Error ? error.message : String(error),
    );
  }
  const pageUrl = new URL(window.location.href);
  const reference = pageUrl.searchParams.get("view");
  const capability = pageUrl.searchParams.get("capability");
  const availableCapabilities = bundle.capabilities ?? [];
  let focused: FocusedView | undefined;
  if (reference || capability) {
    const refresh = pageUrl.searchParams.get("refresh") === "1";
    const requested = reference
      ? `view=${encodeURIComponent(reference)}${refresh ? "&refresh=1" : ""}`
      : `capability=${encodeURIComponent(capability ?? "")}&level=${encodeURIComponent(pageUrl.searchParams.get("level") ?? "capability")}${pageUrl.searchParams.get("root") ? `&root=${encodeURIComponent(pageUrl.searchParams.get("root") ?? "")}` : ""}`;
    const response = await fetch(`./focus.json?${requested}`);
    if (!response.ok) {
      const why = (await response.text()).trim() || `focus.json answered ${response.status}`;
      return unavailable(el, why, response.status === 409 ? reference ?? undefined : undefined);
    }
    try {
      focused = await response.json() as FocusedView;
      if (focused.schema !== "rosetta.focus/v1" || typeof focused.diagram !== "string" || !Array.isArray(focused.nodes)) {
        throw new Error("focus.json is not a focused view");
      }
    } catch (error) {
      return unavailable(el, error instanceof Error ? error.message : String(error));
    }
    bundle = focusedBundle(bundle, focused);
    if (refresh || capability) {
      const url = new URL(window.location.href);
      url.searchParams.set("view", focused.reference);
      url.searchParams.delete("refresh");
      url.searchParams.delete("capability");
      url.searchParams.delete("level");
      url.searchParams.delete("root");
      window.history.replaceState(null, "", url);
    }
  }
  const brand = resolveBrand(given.brand);
  const dark = window.matchMedia("(prefers-color-scheme: dark)");
  const theme = (): AtlasTheme => (dark.matches ? "dark" : "light");
  const atlas = mount(el, { bundle, brand, theme: theme() });
  const requestedSelection = pageUrl.searchParams.get("select");
  const fullContext = !focused && requestedSelection
    ? fullAtlasContext(bundle, requestedSelection)
    : undefined;
  if (fullContext) atlas.open(fullContext.view, fullContext.root);
  const first = fullContext?.root ?? navigationSelection(bundle, [
    requestedSelection,
    focused?.query.roots[0],
    focused?.capabilities[0]?.parts[0],
  ]);
  if (first && bundle.nodes.some((node) => node.id === first)) atlas.select(first);
  const session = given.staticSite
    ? undefined
    : await fetch("./session.json").then(async (response) => response.ok ? await response.json() as LocalSession : undefined).catch(() => undefined);
  mountPanel(atlas, session?.local && session.token ? session : undefined);
  await renderNavigation(atlas, focused, availableCapabilities, Boolean(session?.local));
  if (!given.staticSite) {
    atlas.onPanelChange(() => {
      const selected = atlas.panelState().trail.at(-1);
      const url = new URL(window.location.href);
      if (selected) url.searchParams.set("select", selected);
      else url.searchParams.delete("select");
      if (url.href !== window.location.href) window.history.replaceState(null, "", url);
    });
  }
  const applyTheme = () => {
    document.documentElement.classList.toggle("dark", dark.matches);
    atlas.setTheme(theme());
  };
  applyTheme();
  dark.addEventListener("change", applyTheme);
  document.title = `${bundle.project.name} atlas`;
  return atlas;
}

/** Return the parts this view actually draws, excluding subgraph titles. */
function drawnNodes(source: string): Set<string> {
  const visible = new Set<string>();
  for (const match of source.matchAll(/^\s*click\s+\S+\s+"atlas:([^"]+)"/gm)) {
    try { visible.add(decodeURIComponent(match[1]!)); } catch { /* A malformed link cannot select a part. */ }
  }
  return visible;
}

/** The full Atlas can reopen a part absent from its broad overview by using
 * the part's own Connections view. Its breadcrumb still returns to overview. */
function fullAtlasContext(bundle: AtlasBundle, selected: string): { view: string; root: string } | undefined {
  const overview = bundle.views.find((view) => view.kind === "overview") ?? bundle.views[0];
  if (overview && drawnNodes(overview.sources[overview.default_root] ?? "").has(selected)) return undefined;
  const connections = bundle.views.find((view) => view.kind === "connections");
  if (!connections) return undefined;
  const byId = new Map(bundle.nodes.map((node) => [node.id, node]));
  let id: string | undefined = selected;
  const visited = new Set<string>();
  while (id && !visited.has(id)) {
    if (Object.hasOwn(connections.sources, id)) return { view: connections.id, root: id };
    visited.add(id);
    id = byId.get(id)?.parent;
  }
  return undefined;
}

/** Keep a selected part across a page change, using its drawn parent if needed. */
function navigationSelection(bundle: AtlasBundle, candidates: Array<string | null | undefined>): string | undefined {
  const view = bundle.views.find((candidate) => candidate.kind === "overview") ?? bundle.views[0];
  const source = view?.sources[view.default_root];
  if (!source) return undefined;
  const visible = drawnNodes(source);
  const byId = new Map(bundle.nodes.map((node) => [node.id, node]));
  for (const candidate of candidates) {
    let id = candidate ?? undefined;
    const visited = new Set<string>();
    while (id && !visited.has(id)) {
      if (visible.has(id)) return id;
      visited.add(id);
      id = byId.get(id)?.parent;
    }
  }
  // Start a large overview at a connected part the diagram actually draws.
  const score = new Map([...visible].map((id) => [id, 0]));
  for (const edge of bundle.edges) {
    if (edge.kind === "contains" || !visible.has(edge.from) || !visible.has(edge.to)) continue;
    score.set(edge.from, (score.get(edge.from) ?? 0) + 1);
    score.set(edge.to, (score.get(edge.to) ?? 0) + 1);
  }
  return [...score].sort((left, right) => right[1] - left[1])[0]?.[0];
}

async function renderNavigation(atlas: AtlasHandle, focused: FocusedView | undefined, capabilities: AtlasBundle["capabilities"], local: boolean): Promise<void> {
  const bar = document.getElementById("focus-bar");
  if (!bar) return;
  if (focused) {
    bar.hidden = false;
    const active = focused.capabilities[0];
    const omissions = focused.omissions;
    const hidden = omissions.nodes_over_budget + omissions.edges_over_budget + omissions.edges_filtered + omissions.beyond_depth + omissions.implementation_links_not_drawn;
    const label = document.createElement("span");
    label.className = "rosetta-focus-summary";
    label.textContent = `${active?.name ?? "Focused view"} · ${active?.status ?? "selected"} · ${focused.query.level}${focused.implementation_links.length > 0 ? " · dotted: implementation link" : ""}${hidden > 0 ? ` · ${hidden} omitted or beyond focus` : ""}`;
    const reference = document.createElement("details");
    reference.className = "rosetta-focus-reference";
    const referenceSummary = document.createElement("summary");
    referenceSummary.textContent = `Shared view ${focused.reference.slice(0, 12)}…`;
    const referenceCode = document.createElement("code");
    referenceCode.textContent = focused.reference;
    const copy = document.createElement("button");
    copy.type = "button";
    copy.textContent = "Copy shared view link";
    copy.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(window.location.href);
        copy.textContent = "Link copied";
      } catch {
        copy.textContent = "Copy unavailable; use the address bar";
      }
    });
    reference.append(referenceSummary, referenceCode, copy);
    const controls = document.createElement("span");
    controls.className = "rosetta-focus-controls";
    if (active) {
      const level = document.createElement("select");
      level.setAttribute("aria-label", "Detail level");
      for (const name of ["capability", "component", "implementation"]) {
        const option = document.createElement("option");
        option.value = name;
        option.textContent = name[0]?.toUpperCase() + name.slice(1);
        level.append(option);
      }
      level.value = focused.query.level;
      level.addEventListener("change", () => {
        const url = new URL(window.location.href);
        url.search = "";
        url.searchParams.set("capability", active.id);
        url.searchParams.set("level", level.value);
        const selected = atlas.panelState().trail.at(-1);
        if (selected) {
          url.searchParams.set("root", selected);
          url.searchParams.set("select", selected);
        }
        window.location.href = url.href;
      });
      controls.append(level);
    }
    const link = document.createElement("a");
    link.textContent = "Full map";
    const updateMapLink = () => {
      const url = new URL("./", window.location.href);
      const selected = atlas.panelState().trail.at(-1);
      if (selected) url.searchParams.set("select", selected);
      link.href = url.href;
    };
    atlas.onPanelChange(updateMapLink);
    updateMapLink();
    controls.append(link);
    const details = document.createElement("button");
    details.type = "button";
    details.textContent = "Details & evidence";
    details.addEventListener("click", () => atlas.setPanelOpen(true));
    controls.append(details);
    bar.replaceChildren(label, controls, reference);
    return;
  }
  if (!local) return;
  bar.hidden = false;
  const header = document.createElement("span");
  header.className = "rosetta-overview-header";
  const title = document.createElement("span");
  title.className = "rosetta-focus-summary";
  title.textContent = `${atlas.panelState().bundle.project.name} atlas`;
  header.append(title);
  const controls = document.createElement("span");
  controls.className = "rosetta-focus-controls rosetta-overview-actions";
  const select = document.createElement("select");
  select.setAttribute("aria-label", "Explore a capability");
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.textContent = "Explore a capability";
  select.append(placeholder);
  for (const capability of capabilities ?? []) {
    const option = document.createElement("option");
    option.value = capability.id;
    option.textContent = capability.name;
    select.append(option);
  }
  select.addEventListener("change", () => {
    if (!select.value) return;
    const url = new URL(window.location.href);
    url.search = "";
    url.searchParams.set("capability", select.value);
    url.searchParams.set("level", "capability");
    window.location.href = url.href;
  });
  if (capabilities?.length) header.append(select);
  const focus = document.createElement("button");
  focus.type = "button";
  const updateFocus = () => {
    const { bundle, trail } = atlas.panelState();
    const id = trail.at(-1);
    const node = bundle.nodes.find((candidate) => candidate.id === id);
    const connections = bundle.views.find((view) => view.kind === "connections");
    const count = bundle.edges.filter((edge) => edge.from === id || edge.to === id).length;
    focus.hidden = !id || !node || !connections || !Object.hasOwn(connections.sources, id);
    if (!focus.hidden) focus.textContent = `Focus on ${node!.label} · ${count} connections`;
  };
  focus.addEventListener("click", () => {
    const { bundle, trail } = atlas.panelState();
    const id = trail.at(-1);
    const connections = bundle.views.find((view) => view.kind === "connections");
    if (id && connections && Object.hasOwn(connections.sources, id)) atlas.open(connections.id, id);
  });
  atlas.onPanelChange(updateFocus);
  updateFocus();
  controls.append(focus);
  const details = document.createElement("button");
  details.type = "button";
  details.textContent = "Details";
  details.setAttribute("aria-label", "Open selected part details and evidence");
  details.addEventListener("click", () => atlas.setPanelOpen(true));
  controls.append(details);
  bar.replaceChildren(header, controls);
}

function unavailable(el: HTMLElement, why: string, staleReference?: string): null {
  const notice = document.createElement("p");
  notice.className = "virgil-atlas-unavailable";
  notice.setAttribute("role", "alert");
  notice.textContent = `The atlas could not be read: ${why}`;
  if (staleReference) {
    const refresh = document.createElement("button");
    refresh.type = "button";
    refresh.textContent = "Regenerate this view on the current repository";
    refresh.addEventListener("click", () => {
      const url = new URL(window.location.href);
      url.searchParams.set("view", staleReference);
      url.searchParams.set("refresh", "1");
      window.location.href = url.href;
    });
    el.replaceChildren(notice, refresh);
  } else {
    el.replaceChildren(notice);
  }
  return null;
}

function focusedBundle(bundle: AtlasBundle, view: FocusedView): AtlasBundle {
  return {
    ...bundle,
    views: [{
      id: view.id,
      kind: "custom",
      title: view.capabilities[0]?.name ?? "Focused view",
      description: `The ${view.query.level} selection shared with the agent.`,
      default_root: "",
      sources: { "": view.diagram },
    }],
    nodes: view.nodes,
    edges: view.edges,
    terms: view.terms,
    capabilities: view.capabilities,
    target_exists: view.target_exists,
    snapshot: view.snapshot,
    focused_view_id: view.id,
  };
}

type Served =
  | { readonly bundle: unknown; readonly brand?: Partial<AtlasBrand>; readonly staticSite?: boolean }
  | { readonly failure: string };

async function fetchServed(): Promise<Served> {
  // Consume the bundle body while the brand request is pending. The loopback
  // server handles one connection at a time; waiting for brand before reading
  // a large bundle can stop the browser from draining the first response.
  const [result, brand] = await Promise.all([
    fetch("./bundle.json").then(async (bundle): Promise<Served> => {
      if (!bundle.ok) {
        const text = (await bundle.text()).trim();
        return {
          failure:
            text === ""
              ? `bundle.json answered ${bundle.status} ${bundle.statusText}`.trim()
              : text,
        };
      }
      try {
        return { bundle: await bundle.json() };
      } catch (error) {
        return {
          failure: `bundle.json is not JSON: ${error instanceof Error ? error.message : String(error)}`,
        };
      }
    }).catch((error): Served => ({ failure: `bundle.json: ${error instanceof Error ? error.message : String(error)}` })),
    fetch("./brand.json").then((r) =>
      r.ok ? (r.json() as Promise<Partial<AtlasBrand>>) : undefined,
    ).catch(() => undefined),
  ]);
  if ("failure" in result) return result;
  return brand === undefined ? result : { ...result, brand };
}
