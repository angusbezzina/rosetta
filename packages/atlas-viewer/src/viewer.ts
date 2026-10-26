import {
  type AtlasBrand,
  type AtlasTheme,
  brandVariables,
  cssBrand,
  resolveBrand,
} from "./brand.ts";
import {
  type Mounted,
  mount as peleMount,
  RENDER_OPTIONS,
  type RenderResult,
} from "./pele.ts";
import { flowEnds, relationVerbs } from "./relationship.ts";
import type { AtlasBundle, AtlasNode, AtlasView, Family } from "./types.ts";

export interface MountOptions {
  bundle: AtlasBundle;
  brand?: Partial<AtlasBrand>;
  theme?: AtlasTheme;
  /**
   * Cmd+F / Ctrl+F opens the atlas's find bar while the atlas is mounted and
   * shown. A host that routes Cmd+F itself passes false and calls find(); the
   * atlas then listens for no key outside its find bar.
   */
  captureFind?: boolean;
  /** Called after every selection, for hosts that mirror it. */
  onSelect?: (node: AtlasNode) => void;
}

export interface AtlasHandle {
  /** Show a view at a root (its default root when absent). */
  open(viewId: string, root?: string): void;
  select(nodeId: string): void;
  /**
   * Open the find bar and focus it; with a query, search for it, else keep
   * the bar's current query.
   */
  find(query?: string): void;
  setTheme(theme: AtlasTheme): void;
  setBrand(brand?: Partial<AtlasBrand>): void;
  /** Swap in a newer bundle; a view re-renders only if its source changed. */
  setBundle(bundle: AtlasBundle): void;
  /**
   * The element beside the diagram the detail panel is drawn into. The
   * viewer places it, shows or hides it with the toolbar's toggle and names
   * it after the part it shows, and never draws inside it: the host renders
   * the panel (`./panel`) there from panelState().
   */
  readonly panel: HTMLElement;
  /** What the panel shows now; a new object whenever it changes. */
  panelState(): AtlasPanelState;
  /** Call `listener` after each change to panelState(); answers its removal. */
  onPanelChange(listener: () => void): () => void;
  /**
   * Show a part in the panel as a step of its trail (a Contains row, a trail
   * step, a file's directory); a part already on the trail goes back to it.
   * The canvas selection follows where the part is drawn.
   */
  drill(nodeId: string): void;
  /**
   * Open or close the panel and remember it. The toolbar's button opens a
   * closed panel; the panel's own close button closes it.
   */
  setPanelOpen(open: boolean): void;
  /** Keep touched part outlines stable while changing views or levels. */
  highlight(nodeIds: ReadonlySet<string>): void;
  destroy(): void;
}

/** What the detail panel shows. */
export interface AtlasPanelState {
  readonly bundle: AtlasBundle;
  /**
   * The selection, then each part drilled into from it; the panel shows the
   * last. Empty until a part is selected.
   */
  readonly trail: readonly string[];
}

interface Canvas {
  key: string;
  el: HTMLDivElement;
  mounted: Mounted | undefined;
  source: string;
  /** Atlas id to its node elements, in source order, rebuilt on every render. */
  nodes: Map<string, Element[]>;
  view: Transform;
}

/**
 * Where a canvas's diagram sits in the figure: translated in CSS pixels from
 * the figure's top left, then scaled about its own top left.
 */
interface Transform {
  x: number;
  y: number;
  scale: number;
  /** Placed for the figure's size; unplaced until both have one. */
  placed: boolean;
  /** Panned or zoomed by the reader, so a resize no longer re-places it. */
  moved: boolean;
}

const FAMILIES: readonly Family[] = [
  "surface",
  "service",
  "store",
  "agent",
  "external",
];

/** The furthest in: a 16px label drawn at 64px. */
const MAX_SCALE = 4;
/** One step of the zoom controls and of + and -. */
const ZOOM_STEP = 1.25;
/** Space between a fitted diagram and the figure's edge. */
const FIT_INSET = 16;
/** How much of a diagram a pan always leaves inside the figure. */
const PAN_KEEP = 64;
/** A press that moves further than this is a drag, never a click. */
const DRAG_SLOP = 4;
/** The view menu's gap below its trigger, and its margin above the atlas's foot. */
const MENU_GAP = 6;
const MENU_MARGIN = 12;

const ROOT_CLASS = "virgil-atlas";
const SCHEME = "atlas:";
/** Where the panel toggle's choice is kept, as "open" or "closed". */
const PANEL_KEY = "virgil-atlas:panel";

// On a narrow first visit, give the map the full width. An explicit panel
// choice wins and persists; blocked storage uses the same width default.
const readPanelOpen = (win: Window | null, host: HTMLElement) => {
  const defaultOpen = (host.clientWidth || win?.innerWidth || 1200) > 1000;
  try {
    const saved = win?.localStorage.getItem(PANEL_KEY);
    return saved === "open" ? true : saved === "closed" ? false : defaultOpen;
  } catch {
    return defaultOpen;
  }
};
const writePanelOpen = (win: Window | null, open: boolean) => {
  try {
    win?.localStorage.setItem(PANEL_KEY, open ? "open" : "closed");
  } catch {}
};

const has = (sources: Record<string, string>, key: string) =>
  Object.hasOwn(sources, key);

/** Numbers each mount's panel, so its toggle can name the panel it controls. */
let mounts = 0;

export function mount(el: HTMLElement, options: MountOptions): AtlasHandle {
  const doc = el.ownerDocument;
  const captureFind = options.captureFind ?? true;

  let bundle = options.bundle;
  let theme: AtlasTheme = options.theme ?? "light";
  let brand = resolveBrand({ ...cssBrand(el), ...options.brand });
  let views = new Map<string, AtlasView>();
  let nodes = new Map<string, AtlasNode>();
  let terms = new Map<string, AtlasBundle["terms"][number]>();
  let capabilityTerms = new Map<string, string[]>();

  const push = <T>(map: Map<string, T[]>, key: string, value: T) => {
    const list = map.get(key);
    if (list) list.push(value);
    else map.set(key, [value]);
  };

  const index = () => {
    views = new Map(bundle.views.map((v) => [v.id, v]));
    nodes = new Map(bundle.nodes.map((n) => [n.id, n]));
    terms = new Map(bundle.terms.map((t) => [t.id, t]));
    capabilityTerms = new Map();
    for (const capability of bundle.capabilities ?? []) {
      for (const part of capability.parts) {
        const listed = capabilityTerms.get(part) ?? [];
        capabilityTerms.set(part, [...new Set([...listed, ...capability.terms])]);
      }
    }
  };
  index();

  const state = {
    view: "",
    root: "",
    selected: null as string | null,
    /** The last view opened at its default root: where the crumbs return. */
    back: "",
  };
  let panelState: AtlasPanelState = { bundle, trail: [] };
  const panelListeners = new Set<() => void>();
  let panelOpen = readPanelOpen(doc.defaultView, el);
  let highlighted = new Set<string>();
  const found = {
    open: false,
    matches: [] as string[],
    index: -1,
  };
  const canvases = new Map<string, Canvas>();

  const h = <K extends keyof HTMLElementTagNameMap>(
    tag: K,
    className?: string,
    text?: string,
  ): HTMLElementTagNameMap[K] => {
    const node = doc.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const button = (className: string, text?: string) => {
    const b = h("button", className, text);
    b.type = "button";
    return b;
  };

  // ---- skeleton ------------------------------------------------------------

  el.classList.add(ROOT_CLASS);
  el.setAttribute("aria-label", `Atlas of ${bundle.project.name}`);

  const main = h("div", "virgil-atlas-main");
  const toolbar = h("div", "virgil-atlas-toolbar");
  const crumbs = h("nav", "virgil-atlas-crumbs");
  crumbs.setAttribute("aria-label", "Atlas level");
  const right = h("div", "virgil-atlas-toolbar-right");

  const menuWrap = h("div", "virgil-atlas-viewmenu");
  const menuButton = button("virgil-atlas-viewmenu-btn");
  menuButton.dataset.action = "menu";
  menuButton.setAttribute("aria-haspopup", "menu");
  menuButton.setAttribute("aria-expanded", "false");
  const menuLabel = h("span", "virgil-atlas-viewmenu-label");
  menuButton.append(
    h("span", "virgil-atlas-viewmenu-kicker", "View"),
    menuLabel,
    chevron(doc),
  );
  const menu = h("div", "virgil-atlas-viewmenu-list");
  menu.setAttribute("role", "menu");
  menu.setAttribute("aria-label", "Atlas views");
  menu.hidden = true;
  menuWrap.append(menuButton, menu);
  const panelToggle = button("virgil-atlas-panel-toggle");
  panelToggle.dataset.action = "panel";
  panelToggle.setAttribute("aria-label", "Open detail panel");
  panelToggle.title = "Open detail panel";
  panelToggle.append(panelIcon(doc));
  right.append(menuWrap, panelToggle);
  toolbar.append(crumbs, right);

  const findBar = h("div", "virgil-atlas-find");
  findBar.setAttribute("role", "search");
  findBar.hidden = true;
  const findInput = h("input", "virgil-atlas-find-input");
  findInput.type = "text";
  findInput.placeholder = "Find a node, path or term";
  findInput.setAttribute("aria-label", "Find a node, path or term");
  findInput.autocomplete = "off";
  findInput.spellcheck = false;
  const findCount = h("span", "virgil-atlas-find-count");
  findCount.setAttribute("aria-live", "polite");
  const findClose = button("virgil-atlas-find-close", "Esc");
  findClose.dataset.action = "find-close";
  findClose.setAttribute("aria-label", "Close find");
  findBar.append(searchIcon(doc), findInput, findCount, findClose);

  const figure = h("figure", "virgil-atlas-figure");
  // The figure is the diagram's one tab stop besides its parts: + and - zoom
  // and 0 fits while it or a part has focus.
  figure.tabIndex = 0;
  figure.setAttribute("aria-label", "Diagram");
  const narrowHint = h("figcaption", "virgil-atlas-pan-hint");
  narrowHint.append(h("span", "", "Drag to explore"));
  const narrowFit = button("virgil-atlas-pan-fit", "Fit whole map");
  narrowFit.dataset.action = "zoom-fit";
  narrowHint.append(narrowFit);
  const zoomBar = h("div", "virgil-atlas-zoom");
  zoomBar.setAttribute("role", "group");
  zoomBar.setAttribute("aria-label", "Zoom");
  const zoomButton = (action: string, label: string, glyph: SVGSVGElement) => {
    const b = button("virgil-atlas-zoom-btn");
    b.dataset.action = action;
    b.setAttribute("aria-label", label);
    b.title = label;
    b.append(glyph);
    zoomBar.append(b);
    return b;
  };
  const zoomIn = zoomButton("zoom-in", "Zoom in (+)", plusIcon(doc));
  const zoomOut = zoomButton("zoom-out", "Zoom out (-)", minusIcon(doc));
  zoomButton("zoom-fit", "Fit (0)", fitIcon(doc));
  const modeButton = button("virgil-atlas-mode-btn");
  modeButton.hidden = true;
  const connectionList = h("div", "virgil-atlas-connection-list");
  connectionList.hidden = true;
  figure.append(zoomBar, narrowHint, modeButton, connectionList);
  const legend = h("div", "virgil-atlas-legend");
  for (const family of FAMILIES) {
    const item = h("span");
    item.append(h("i", `virgil-atlas-swatch fam-${family}`), title(family));
    legend.append(item);
  }
  legend.append(h("span", "virgil-atlas-direction-key", "Arrows show flow. Reads point from data source to reader; writes point to destination."));
  main.append(toolbar, findBar, figure, legend);

  const detail = h("aside", "virgil-atlas-detail");
  detail.id = `virgil-atlas-detail-${++mounts}`;
  panelToggle.setAttribute("aria-controls", detail.id);

  el.replaceChildren(main, detail);

  // The toolbar button only opens: while the panel is open it is gone, and
  // the panel's own close button closes it.
  const showPanel = (open: boolean) => {
    panelOpen = open;
    detail.hidden = !open;
    el.classList.toggle("is-panel-closed", !open);
    panelToggle.hidden = open;
  };
  const setPanelOpen = (open: boolean) => {
    if (open === panelOpen) return;
    const hadFocus = detail.contains(doc.activeElement);
    showPanel(open);
    writePanelOpen(doc.defaultView, open);
    // Focus follows to whichever control now opens or closes it.
    if (!open && hadFocus) panelToggle.focus();
  };
  showPanel(panelOpen);

  // ---- colours -------------------------------------------------------------

  const applyVariables = () => {
    for (const [name, value] of Object.entries(brandVariables(brand, theme))) {
      el.style.setProperty(name, value);
    }
    // The theme the host gave, said to the browser too, so the menu's and
    // the panel's native scrollbars follow it inside a host whose root is
    // another theme.
    el.style.colorScheme = theme;
  };
  applyVariables();

  // ---- views ---------------------------------------------------------------

  /** A view is in the menu iff it draws something at its own default root. */
  const listed = () =>
    bundle.views.filter((v) => has(v.sources, v.default_root));

  const renderMenu = () => {
    menu.replaceChildren(
      ...listed().map((view) => {
        const item = button("virgil-atlas-viewmenu-item");
        item.setAttribute("role", "menuitemradio");
        item.setAttribute("aria-checked", String(view.id === state.view));
        item.dataset.action = "view";
        item.dataset.view = view.id;
        item.append(
          h("span", "virgil-atlas-viewmenu-name", view.title),
          h("span", "virgil-atlas-viewmenu-desc", view.description),
        );
        return item;
      }),
    );
  };
  const menuItems = () => [
    ...menu.querySelectorAll<HTMLButtonElement>(".virgil-atlas-viewmenu-item"),
  ];
  /**
   * The list scrolls within the space below its trigger: the atlas clips what
   * overflows it, so a list taller than that would hide its last views.
   */
  const boundMenu = () => {
    const frame = el.getBoundingClientRect();
    if (frame.height === 0) return;
    const foot = Math.min(
      frame.bottom,
      doc.defaultView?.innerHeight ?? frame.bottom,
    );
    const top = menuButton.getBoundingClientRect().bottom + MENU_GAP;
    menu.style.maxHeight = `${Math.max(foot - top - MENU_MARGIN, 0)}px`;
  };
  const setMenu = (open: boolean) => {
    menu.hidden = !open;
    menuButton.setAttribute("aria-expanded", String(open));
    if (open) {
      boundMenu();
      const items = menuItems();
      (
        items.find((i) => i.getAttribute("aria-checked") === "true") ?? items[0]
      )?.focus();
    }
  };

  const atlasId = (href: string): string | null => {
    if (!href.startsWith(SCHEME)) return null;
    // Pele decodes the href it was given; an id that holds an escape needs
    // the second decode.
    const raw = href.slice(SCHEME.length);
    if (nodes.has(raw)) return raw;
    try {
      const decoded = decodeURIComponent(raw);
      return nodes.has(decoded) ? decoded : null;
    } catch {
      return null;
    }
  };

  // Only a node with an atlas link becomes a tab stop: a subgraph is a
  // cluster, never a node, and an overflow node carries no click.
  const decorate = (canvas: Canvas, result: RenderResult) => {
    canvas.nodes = new Map();
    const byPeleId = new Map<string, Element>();
    for (const g of canvas.el.querySelectorAll(".pele-node[data-id]")) {
      byPeleId.set(g.getAttribute("data-id") ?? "", g);
    }
    for (const link of result.links) {
      const id = atlasId(link.href);
      const g = byPeleId.get(link.id);
      if (!id || !g) continue;
      const node = nodes.get(id);
      g.setAttribute("data-atlas-id", id);
      g.setAttribute("tabindex", "0");
      g.setAttribute("role", "button");
      g.setAttribute("aria-label", node?.label ?? id);
      // The group is the one tab stop; the link inside it stays a link only
      // so Pele's output is unchanged.
      for (const a of g.querySelectorAll("a")) a.setAttribute("tabindex", "-1");
      push(canvas.nodes, id, g);
    }
    mark(canvas);
    if (canvas !== current()) return;
    // A new size for a diagram the reader has not moved is placed afresh.
    if (!canvas.view.moved) place(canvas);
    reveal({ centre: false, glide: false });
  };

  const mark = (canvas: Canvas) => {
    const finding = found.open && findInput.value.trim() !== "";
    canvas.el.classList.toggle("is-finding", finding);
    const matches = new Set(found.matches);
    const peleToAtlas = new Map<string, string>();
    const neighbours = new Set<string>();
    for (const [id, elements] of canvas.nodes) {
      for (const element of elements) {
        const peleId = element.getAttribute("data-id");
        if (peleId) peleToAtlas.set(peleId, id);
      }
    }
    const selectedVisible = state.selected && canvas.nodes.has(state.selected) ? state.selected : undefined;
    for (const element of canvas.el.querySelectorAll<SVGGElement>(".pele-edge[data-id], .pele-edge-label[data-id]")) {
      const match = /^L_(n\d+)_(n\d+)_\d+$/.exec(element.dataset.id ?? "");
      const from = match && peleToAtlas.get(match[1]!);
      const to = match && peleToAtlas.get(match[2]!);
      const incoming = Boolean(selectedVisible && to === selectedVisible);
      const outgoing = Boolean(selectedVisible && from === selectedVisible);
      element.classList.toggle("is-connection-in", incoming);
      element.classList.toggle("is-connection-out", outgoing);
      element.classList.toggle("is-connection-muted", Boolean(selectedVisible && !incoming && !outgoing));
      if (incoming && from) neighbours.add(from);
      if (outgoing && to) neighbours.add(to);
    }
    for (const [id, elements] of canvas.nodes) {
      const selected = id === state.selected;
      const match = finding && matches.has(id);
      for (const g of elements) {
        g.classList.toggle("is-selected", selected);
        g.setAttribute("aria-pressed", String(selected));
        g.classList.toggle("is-match", match);
        g.classList.toggle("is-change-highlighted", highlighted.has(id));
        g.classList.toggle("is-connection-neighbour", neighbours.has(id));
      }
    }
  };

  const showError = (canvas: Canvas, error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    canvas.el.replaceChildren(
      h("p", "virgil-atlas-error", `This view did not render: ${message}`),
    );
  };

  const canvasFor = (viewId: string, root: string, source: string) => {
    const key = `${viewId}\u0000${root}`;
    const existing = canvases.get(key);
    if (existing) {
      if (existing.source !== source) {
        existing.source = source;
        existing.mounted?.update(source);
      }
      return existing;
    }
    const canvas: Canvas = {
      key,
      el: h("div", "virgil-atlas-canvas"),
      mounted: undefined,
      source,
      nodes: new Map(),
      view: { x: 0, y: 0, scale: 1, placed: false, moved: false },
    };
    canvases.set(key, canvas);
    figure.append(canvas.el);
    canvas.mounted = peleMount(canvas.el, source, {
      ...RENDER_OPTIONS,
      onRender: (result) => decorate(canvas, result),
      onError: (error) => showError(canvas, error),
    });
    return canvas;
  };

  const current = () => canvases.get(`${state.view}\u0000${state.root}`);

  let connectionMode: "auto" | "list" | "diagram" = "auto";
  const renderConnectionList = () => {
    const view = views.get(state.view);
    const focusId = view?.kind === "connections"
      ? state.root
      : state.selected ?? bundle.capabilities?.[0]?.parts[0];
    const focus = nodes.get(focusId ?? "");
    if (!focus) return connectionList.replaceChildren();
    const touching = bundle.edges.filter((edge) => edge.from === focus.id || edge.to === focus.id);
    const intro = h("p", "virgil-atlas-connection-intro", `${focus.label} · ${touching.length} relationships. Arrows show flow; each sentence names the acting part.`);
    const bySide = {
      incoming: [] as typeof touching,
      outgoing: [] as typeof touching,
      loop: [] as typeof touching,
    };
    for (const edge of touching) {
      const flow = flowEnds(edge);
      if (flow.from === focus.id && flow.to === focus.id) bySide.loop.push(edge);
      else if (flow.to === focus.id) bySide.incoming.push(edge);
      else bySide.outgoing.push(edge);
    }
    const sections: HTMLElement[] = [];
    for (const [title, edges] of [
      [`Into ${focus.label}`, bySide.incoming],
      [`From ${focus.label}`, bySide.outgoing],
      [`Within ${focus.label}`, bySide.loop],
    ] as const) {
      if (edges.length === 0) continue;
      const section = h("section");
      section.append(h("h3", "", `${title} (${edges.length})`));
      const items = h("ol");
      for (const edge of edges) {
        const flow = flowEnds(edge);
        const actor = nodes.get(edge.from)?.label ?? edge.from;
        const target = nodes.get(edge.to)?.label ?? edge.to;
        const item = h("li");
        const line = h("div", "virgil-atlas-connection-flow");
        for (const [id, label] of [[flow.from, nodes.get(flow.from)?.label ?? flow.from], [flow.to, nodes.get(flow.to)?.label ?? flow.to]] as const) {
          if (line.childElementCount > 0) line.append(h("span", "virgil-atlas-connection-arrow", "→"));
          const part = button("virgil-atlas-connection-part", label);
          if (id === focus.id) part.classList.add("is-focus");
          part.addEventListener("click", () => activate(id));
          line.append(part);
        }
        const sentence = `${actor} ${relationVerbs[edge.kind]} ${target}${edge.label ? ` · ${edge.label}` : ""}`;
        const basis = edge.evidence?.basis.replaceAll("_", " ") ?? (edge.derived ? "derived source" : "declared intent");
        const source = edge.evidence?.source ?? edge.derived?.source ?? "atlas model";
        item.append(line, h("p", "virgil-atlas-connection-sentence", sentence), h("small", "virgil-atlas-connection-source", `${basis} · ${source}`));
        items.append(item);
      }
      section.append(items);
      sections.push(section);
    }
    if (view?.kind === "custom") {
      const targets = (bundle.capabilities ?? [])
        .flatMap((capability) => capability.implementation)
        .filter((target) => target.part === focus.id);
      if (targets.length > 0) {
        const section = h("section");
        section.append(h("h3", "", `Implementation (${targets.length})`));
        const items = h("ol");
        for (const target of targets) {
          const item = h("li");
          const file = bundle.nodes.find((node) => node.path === target.path);
          if (file) {
            const link = button("virgil-atlas-connection-part", target.path);
            link.addEventListener("click", () => activate(file.id));
            item.append(link);
          } else {
            item.append(h("code", "virgil-atlas-implementation-path", target.path));
          }
          const exists = bundle.target_exists?.[target.path];
          item.append(h("small", "virgil-atlas-connection-source", exists === true ? "Path exists in this snapshot" : exists === false ? "Path missing in this snapshot" : "Path not checked"));
          items.append(item);
        }
        section.append(items);
        sections.push(section);
      }
    }
    connectionList.replaceChildren(intro, ...sections);
  };

  const updateConnectionMode = () => {
    const view = views.get(state.view);
    const hasList = (view?.kind === "connections" && Boolean(state.root))
      || (view?.kind === "custom" && bundle.focused_view_id === view.id);
    const width = figure.clientWidth || el.clientWidth || doc.defaultView?.innerWidth || 1200;
    const list = hasList && (connectionMode === "list" || (connectionMode === "auto" && width <= 500));
    figure.classList.toggle("is-connection-list", Boolean(list));
    connectionList.hidden = !list;
    modeButton.hidden = !hasList;
    modeButton.textContent = list ? "Diagram" : "Readable list";
    if (list) renderConnectionList();
  };
  modeButton.addEventListener("click", () => {
    connectionMode = figure.classList.contains("is-connection-list") ? "diagram" : "list";
    updateConnectionMode();
  });

  /**
   * Project / the view, at a view's default root; Project / the view the
   * reader came from / the root's label, at any other root.
   */
  const renderCrumbs = () => {
    const view = views.get(state.view);
    if (!view) return;
    const home = listed()[0];
    const atHome =
      home !== undefined &&
      home.id === view.id &&
      home.default_root === state.root;
    const steps: Array<[string, string, string] | [string]> = [
      home && !atHome
        ? [bundle.project.name, home.id, home.default_root]
        : [bundle.project.name],
    ];
    if (state.root === view.default_root) {
      steps.push([view.title]);
    } else {
      const back = views.get(state.back) ?? home;
      if (back) steps.push([back.title, back.id, back.default_root]);
      steps.push([nodes.get(state.root)?.label ?? state.root]);
    }
    const parts: Node[] = [];
    steps.forEach((step, i) => {
      if (i > 0) parts.push(h("span", "virgil-atlas-crumb-sep", "/"));
      if (step.length === 3) {
        const link = button("virgil-atlas-crumb-link", step[0]);
        link.dataset.action = "crumb";
        link.dataset.view = step[1];
        link.dataset.root = step[2];
        parts.push(link);
      } else {
        const here = h("span", "virgil-atlas-crumb-here", step[0]);
        here.setAttribute("aria-current", "location");
        parts.push(here);
      }
    });
    crumbs.replaceChildren(...parts);
  };

  const open = (viewId: string, root?: string) => {
    const view = views.get(viewId);
    if (!view) throw new Error(`atlas: no view "${viewId}"`);
    const at = root ?? view.default_root;
    const source = view.sources[at];
    if (source === undefined || !has(view.sources, at)) {
      throw new Error(`atlas: view "${viewId}" has no source for "${at}"`);
    }
    state.view = viewId;
    state.root = at;
    if (view.kind !== "connections" && !(view.kind === "custom" && bundle.focused_view_id === view.id)) connectionMode = "auto";
    if (at === view.default_root) state.back = viewId;
    const canvas = canvasFor(viewId, at, source);
    for (const other of canvases.values()) other.el.hidden = other !== canvas;
    canvas.view.moved = false;
    place(canvas);
    // A part's Connections is named for the part: "CI Connections".
    const about = nodes.get(at);
    menuLabel.textContent =
      view.kind === "connections" && about
        ? `${about.label} ${view.title}`
        : view.title;
    for (const item of menuItems()) {
      item.setAttribute("aria-checked", String(item.dataset.view === viewId));
    }
    renderCrumbs();
    updateConnectionMode();
    // A root that is a node is what the view is about; otherwise keep the
    // selection when the view draws it, else take its first node.
    if (at !== view.default_root && nodes.has(at)) {
      state.selected = at;
    } else if (!state.selected || !canvas.nodes.has(state.selected)) {
      state.selected = canvas.nodes.keys().next().value ?? state.selected;
    }
    if (found.open) runFind();
    if (found.matches.length === 0 && state.selected) {
      select(state.selected, { centre: false, glide: false });
    } else mark(canvas);
  };

  // ---- pan and zoom --------------------------------------------------------

  /** The diagram's own size, as Pele drew it; undefined before it has one. */
  const natural = (canvas: Canvas) => {
    const svg = canvas.el.querySelector(":scope > svg");
    const width = Number(svg?.getAttribute("width")) || 0;
    const height = Number(svg?.getAttribute("height")) || 0;
    return width > 0 && height > 0 ? { width, height } : undefined;
  };

  /** The scale that shows the whole diagram inside the figure's inset. */
  const fitScale = (canvas: Canvas) => {
    const size = natural(canvas);
    const width = figure.clientWidth - 2 * FIT_INSET;
    const height = figure.clientHeight - 2 * FIT_INSET;
    if (!size || width <= 0 || height <= 0) return undefined;
    return Math.min(width / size.width, height / size.height);
  };

  /** Out as far as the whole diagram, and to its own size when smaller. */
  const minScale = (canvas: Canvas) => Math.min(fitScale(canvas) ?? 1, 1);

  /**
   * Write the transform, first keeping PAN_KEEP pixels of the diagram (or
   * all of a smaller one) inside the figure on each axis.
   */
  const apply = (canvas: Canvas, glide = false) => {
    const view = canvas.view;
    const size = natural(canvas);
    if (size) {
      const keep = (extent: number, frame: number, at: number) => {
        const drawn = extent * view.scale;
        const inside = Math.min(PAN_KEEP, drawn);
        return Math.min(Math.max(at, inside - drawn), frame - inside);
      };
      view.x = keep(size.width, figure.clientWidth, view.x);
      view.y = keep(size.height, figure.clientHeight, view.y);
    }
    canvas.el.classList.toggle("is-gliding", glide);
    canvas.el.style.transform = `translate(${view.x}px, ${view.y}px) scale(${view.scale})`;
    zoomIn.disabled = view.scale >= MAX_SCALE;
    zoomOut.disabled = view.scale <= minScale(canvas);
  };

  /**
   * Show the diagram at a scale: centred on an axis where it fits, from the
   * inset on one where it does not, so a large view opens at its start.
   */
  const placeAt = (canvas: Canvas, scale: number) => {
    const size = natural(canvas);
    if (!size) return;
    const axis = (frame: number, extent: number) => {
      const drawn = extent * scale;
      return drawn <= frame - 2 * FIT_INSET ? (frame - drawn) / 2 : FIT_INSET;
    };
    canvas.view.scale = scale;
    canvas.view.x = axis(figure.clientWidth, size.width);
    canvas.view.y = axis(figure.clientHeight, size.height);
    canvas.view.placed = true;
    apply(canvas);
  };

  /** Keep labels near their drawn size when fitting would make them tiny. */
  const place = (canvas: Canvas) => {
    const fit = fitScale(canvas);
    if (fit === undefined) {
      canvas.view.placed = false;
      return;
    }
    // A focused Connections view has only the selected part and its immediate
    // neighbours. At a moderate fit, show all of them instead of clipping the
    // outgoing side; a sprawling overview still opens at readable text size.
    const connections = bundle.views.find((view) => view.id === state.view)?.kind === "connections";
    const readable = fit < (connections ? 0.7 : 0.8);
    narrowHint.hidden = !readable;
    placeAt(canvas, readable ? (figure.clientWidth < 720 ? 1 : 0.85) : Math.min(fit, 1));
  };

  const fit = () => {
    const canvas = current();
    const scale = canvas && fitScale(canvas);
    if (!canvas || scale === undefined) return;
    canvas.view.moved = true;
    placeAt(canvas, Math.min(scale, 1));
  };

  /** Zoom keeping the diagram's point under (clientX, clientY) where it is. */
  const zoomAt = (factor: number, clientX?: number, clientY?: number) => {
    const canvas = current();
    if (!canvas?.view.placed) return;
    const view = canvas.view;
    const next = Math.min(
      Math.max(view.scale * factor, minScale(canvas)),
      MAX_SCALE,
    );
    if (next === view.scale) return;
    const frame = figure.getBoundingClientRect();
    const fx =
      clientX === undefined
        ? figure.clientWidth / 2
        : clientX - frame.left - figure.clientLeft;
    const fy =
      clientY === undefined
        ? figure.clientHeight / 2
        : clientY - frame.top - figure.clientTop;
    view.x = fx - ((fx - view.x) / view.scale) * next;
    view.y = fy - ((fy - view.y) / view.scale) * next;
    view.scale = next;
    view.moved = true;
    apply(canvas);
  };

  const panBy = (dx: number, dy: number) => {
    const canvas = current();
    if (!canvas?.view.placed) return;
    canvas.view.x += dx;
    canvas.view.y += dy;
    canvas.view.moved = true;
    apply(canvas);
  };

  // ---- selection and the panel ---------------------------------------------

  /**
   * How a selection is brought into view: centred always (a find step) or
   * only when it is not wholly in view, and moved there or jumped.
   */
  interface Reveal {
    centre: boolean;
    glide: boolean;
  }
  const NEAR: Reveal = { centre: false, glide: true };

  const select = (id: string, how: Reveal = NEAR) => {
    if (!nodes.has(id)) return;
    setSelected(id, how);
    setTrail([id]);
  };

  const setTrail = (trail: readonly string[]) => {
    panelState = { bundle, trail };
    const shown = nodes.get(trail[trail.length - 1] ?? "");
    if (shown) detail.setAttribute("aria-label", shown.label);
    else detail.removeAttribute("aria-label");
    for (const listener of panelListeners) listener();
  };

  const setSelected = (id: string, how: Reveal = NEAR) => {
    state.selected = id;
    const canvas = current();
    if (canvas) mark(canvas);
    if (figure.classList.contains("is-connection-list")) renderConnectionList();
    reveal(how);
    const node = nodes.get(id);
    if (node) options.onSelect?.(node);
  };

  /**
   * Pan so the selected part is centred in the figure: a view opened at a
   * root, or a part found, may be off screen.
   */
  const reveal = (how: Reveal) => {
    const canvas = current();
    const g = state.selected
      ? canvas?.nodes.get(state.selected)?.[0]
      : undefined;
    if (!canvas?.view.placed || !g) return;
    const box = g.getBoundingClientRect();
    // Not laid out yet (hidden, or Pele has not drawn it): decorate asks again.
    if (box.width === 0 && box.height === 0) return;
    const frame = figure.getBoundingClientRect();
    const inView =
      box.left >= frame.left &&
      box.right <= frame.right &&
      box.top >= frame.top &&
      box.bottom <= frame.bottom;
    if (inView && !how.centre) return;
    // In a narrow left-to-right diagram, leave room to read the incoming
    // labels beside the selected part. Explicit Find still centres exactly.
    const horizontal = !how.centre && frame.width < 720 ? 0.75 : 0.5;
    canvas.view.x += frame.left + frame.width * horizontal - (box.left + box.width / 2);
    canvas.view.y += frame.top + frame.height / 2 - (box.top + box.height / 2);
    apply(canvas, how.glide);
  };

  /**
   * Select, then open what the node leads to: its own root in a view with
   * roots (a Records area), else its Connections when it has any.
   */
  const activate = (id: string) => {
    select(id);
    const view = views.get(state.view);
    if (view && id !== state.root && has(view.sources, id)) {
      open(view.id, id);
      return;
    }
    const connections = bundle.views.find((v) => v.kind === "connections");
    if (
      connections &&
      has(connections.sources, id) &&
      !(state.view === connections.id && state.root === id)
    ) {
      open(connections.id, id);
    }
  };

  const drill = (id: string) => {
    if (!nodes.has(id)) return;
    const { trail } = panelState;
    const at = trail.indexOf(id);
    if (current()?.nodes.has(id)) setSelected(id);
    setTrail(at >= 0 ? trail.slice(0, at + 1) : [...trail, id]);
  };

  // ---- find ----------------------------------------------------------------

  const haystack = (id: string) => {
    const node = nodes.get(id);
    if (!node) return "";
    const relatedTerms = new Set([
      ...(node.refs.term ? [node.refs.term] : []),
      ...(capabilityTerms.get(id) ?? []),
    ]);
    const words = [...relatedTerms].flatMap((termId) => {
      const term = terms.get(termId);
      return term ? [term.name, ...term.avoid.map((item) => item.word)] : [];
    });
    return [node.label, node.path, ...words]
      .filter(Boolean)
      .join("\n")
      .toLowerCase();
  };

  const showMatch = () => {
    const total = found.matches.length;
    findCount.textContent =
      findInput.value.trim() === ""
        ? ""
        : total === 0
          ? "No matches"
          : `${found.index + 1} of ${total}`;
    const id = found.matches[found.index];
    if (id) select(id, { centre: true, glide: true });
    else {
      const canvas = current();
      if (canvas) mark(canvas);
    }
  };

  // Matches are the canvas's clickable nodes, so a subgraph title never is.
  const runFind = () => {
    const query = findInput.value.trim().toLowerCase();
    const canvas = current();
    found.matches =
      found.open && query !== "" && canvas
        ? [...canvas.nodes.keys()].filter((id) => haystack(id).includes(query))
        : [];
    found.index = found.matches.length > 0 ? 0 : -1;
    showMatch();
  };

  const stepFind = (delta: number) => {
    const total = found.matches.length;
    if (total === 0) return;
    found.index = (found.index + delta + total) % total;
    showMatch();
  };

  const openFind = (query?: string) => {
    found.open = true;
    findBar.hidden = false;
    if (query !== undefined) findInput.value = query;
    findInput.focus();
    findInput.select();
    runFind();
  };

  // Focus goes back to what find left selected, so the reader's place in
  // the diagram is where the keyboard is.
  const closeFind = () => {
    found.open = false;
    findBar.hidden = true;
    found.matches = [];
    found.index = -1;
    findCount.textContent = "";
    const canvas = current();
    if (canvas) mark(canvas);
    const selected = state.selected
      ? canvas?.nodes.get(state.selected)?.[0]
      : undefined;
    if (selected instanceof SVGElement || selected instanceof HTMLElement) {
      selected.focus();
    } else {
      menuButton.focus();
    }
  };

  // ---- events --------------------------------------------------------------

  const nodeOf = (target: EventTarget | null) =>
    target instanceof Element
      ? target.closest("[data-atlas-id]")?.getAttribute("data-atlas-id")
      : undefined;

  const onClick = (event: MouseEvent) => {
    const id = nodeOf(event.target);
    if (id && figure.contains(event.target as Node)) {
      // A node is a link to atlas:<id>; following it would leave the page.
      event.preventDefault();
      activate(id);
      return;
    }
    const target = (event.target as Element | null)?.closest<HTMLElement>(
      "[data-action]",
    );
    switch (target?.dataset.action) {
      case "menu":
        setMenu(menuButton.getAttribute("aria-expanded") !== "true");
        return;
      case "view":
        setMenu(false);
        menuButton.focus();
        open(target.dataset.view ?? "");
        return;
      case "crumb":
        open(target.dataset.view ?? "", target.dataset.root);
        return;
      case "find-close":
        closeFind();
        return;
      case "zoom-in":
        zoomAt(ZOOM_STEP);
        return;
      case "zoom-out":
        zoomAt(1 / ZOOM_STEP);
        return;
      case "zoom-fit":
        fit();
        return;
      case "panel":
        setPanelOpen(true);
        return;
    }
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.target === findInput) {
      if (event.key === "Enter") {
        event.preventDefault();
        stepFind(event.shiftKey ? -1 : 1);
      } else if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        closeFind();
      }
      return;
    }
    if (
      !menu.hidden &&
      (event.key === "ArrowDown" ||
        event.key === "ArrowUp" ||
        event.key === "Escape")
    ) {
      event.preventDefault();
      if (event.key === "Escape") {
        setMenu(false);
        menuButton.focus();
        return;
      }
      const items = menuItems();
      const at = items.indexOf(doc.activeElement as HTMLButtonElement);
      const next =
        event.key === "ArrowDown"
          ? Math.min(items.length - 1, at + 1)
          : Math.max(0, at - 1);
      items[next]?.focus();
      return;
    }
    if (event.key === "Escape" && found.open) {
      event.preventDefault();
      closeFind();
      return;
    }
    if (figure.classList.contains("is-connection-list") && figure.contains(event.target as Node)) return;
    if (
      figure.contains(event.target as Node) &&
      !(event.metaKey || event.ctrlKey || event.altKey)
    ) {
      const zoom =
        event.key === "+" || event.key === "="
          ? () => zoomAt(ZOOM_STEP)
          : event.key === "-"
            ? () => zoomAt(1 / ZOOM_STEP)
            : event.key === "0"
              ? fit
              : undefined;
      if (zoom) {
        event.preventDefault();
        zoom();
        return;
      }
    }
    if (event.key !== "Enter" && event.key !== " ") return;
    const id = nodeOf(event.target);
    if (!id || !figure.contains(event.target as Node)) return;
    event.preventDefault();
    activate(id);
  };

  /**
   * A press on the diagram that moves past DRAG_SLOP pans it, and the click
   * its release would send is swallowed, so a drag that starts or ends on a
   * part never opens it.
   */
  const drag = {
    pointer: -1,
    x: 0,
    y: 0,
    moved: false,
    swallowClick: false,
  };

  const onPointerDown = (event: PointerEvent) => {
    drag.swallowClick = false;
    if (figure.classList.contains("is-connection-list") || modeButton.contains(event.target as Node)) return;
    if (event.button !== 0 || zoomBar.contains(event.target as Node) || narrowHint.contains(event.target as Node)) return;
    drag.pointer = event.pointerId;
    drag.x = event.clientX;
    drag.y = event.clientY;
    drag.moved = false;
    current()?.el.classList.remove("is-gliding");
  };

  const onPointerMove = (event: PointerEvent) => {
    if (event.pointerId !== drag.pointer) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    if (!drag.moved) {
      if (Math.hypot(dx, dy) <= DRAG_SLOP) return;
      drag.moved = true;
      figure.setPointerCapture?.(event.pointerId);
      figure.classList.add("is-panning");
    }
    drag.x = event.clientX;
    drag.y = event.clientY;
    panBy(dx, dy);
  };

  const onPointerUp = (event: PointerEvent) => {
    if (event.pointerId !== drag.pointer) return;
    drag.pointer = -1;
    drag.swallowClick = drag.moved;
    figure.classList.remove("is-panning");
  };

  const onFigureClick = (event: MouseEvent) => {
    if (!drag.swallowClick) return;
    drag.swallowClick = false;
    event.preventDefault();
    event.stopPropagation();
  };

  // A part's link would otherwise start the browser's own link drag.
  const onDragStart = (event: DragEvent) => event.preventDefault();

  /**
   * The wheel zooms about the cursor; a trackpad pinch arrives as a wheel
   * with ctrlKey and much smaller deltas, so it takes a larger rate.
   */
  const onWheel = (event: WheelEvent) => {
    if (figure.classList.contains("is-connection-list")) return;
    event.preventDefault();
    current()?.el.classList.remove("is-gliding");
    const lines = event.deltaMode === 1 ? 16 : 1;
    const delta = Math.min(Math.max(event.deltaY * lines, -50), 50);
    zoomAt(
      2 ** (-delta * (event.ctrlKey ? 0.02 : 0.004)),
      event.clientX,
      event.clientY,
    );
  };

  const onFigureResize = () => {
    updateConnectionMode();
    const canvas = current();
    if (!canvas) return;
    if (!canvas.view.moved) {
      place(canvas);
      reveal({ centre: false, glide: false });
    } else if (canvas.view.placed) {
      apply(canvas);
    }
  };
  const resize = doc.defaultView?.ResizeObserver
    ? new doc.defaultView.ResizeObserver(onFigureResize)
    : undefined;
  resize?.observe(figure);

  const shown = () => el.isConnected && !el.closest("[hidden]");

  // Cmd+F / Ctrl+F belongs to the atlas only while it handles the key; the
  // browser's find stays everywhere else.
  const onDocumentKeyDown = (event: KeyboardEvent) => {
    if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey)
      return;
    if (event.key.toLowerCase() !== "f") return;
    if (!shown()) return;
    event.preventDefault();
    openFind();
  };

  const onDocumentClick = (event: MouseEvent) => {
    if (
      !menu.hidden &&
      !(event.target instanceof Node && menuWrap.contains(event.target))
    ) {
      setMenu(false);
    }
  };

  el.addEventListener("click", onClick);
  el.addEventListener("keydown", onKeyDown);
  figure.addEventListener("pointerdown", onPointerDown);
  figure.addEventListener("pointermove", onPointerMove);
  figure.addEventListener("pointerup", onPointerUp);
  figure.addEventListener("pointercancel", onPointerUp);
  figure.addEventListener("click", onFigureClick, { capture: true });
  figure.addEventListener("dragstart", onDragStart);
  figure.addEventListener("wheel", onWheel, { passive: false });
  findInput.addEventListener("input", runFind);
  // A host that routes Cmd+F itself calls find(); a second listener here
  // would answer the same press twice.
  if (captureFind) doc.addEventListener("keydown", onDocumentKeyDown);
  doc.addEventListener("click", onDocumentClick);

  const openHome = () => {
    const home = listed()[0];
    if (home) open(home.id);
  };

  renderMenu();
  openHome();

  return {
    open,
    select: (nodeId) => select(nodeId),
    panel: detail,
    panelState: () => panelState,
    onPanelChange(listener) {
      panelListeners.add(listener);
      return () => panelListeners.delete(listener);
    },
    drill,
    setPanelOpen,
    highlight(nodeIds) {
      highlighted = new Set(nodeIds);
      for (const canvas of canvases.values()) mark(canvas);
    },
    find: (query) => openFind(query),
    setTheme(next) {
      theme = next;
      applyVariables();
    },
    setBrand(next) {
      brand = resolveBrand({ ...cssBrand(el), ...next });
      applyVariables();
    },
    setBundle(next) {
      bundle = next;
      index();
      renderMenu();
      for (const [key, canvas] of canvases) {
        const [viewId, root] = key.split("\u0000") as [string, string];
        const view = views.get(viewId);
        const source =
          view && has(view.sources, root) ? view.sources[root] : undefined;
        if (source === undefined) {
          canvas.mounted?.destroy();
          canvas.el.remove();
          canvases.delete(key);
        } else if (source !== canvas.source) {
          canvas.source = source;
          canvas.mounted?.update(source);
        }
      }
      if (state.selected && !nodes.has(state.selected)) state.selected = null;
      if (!views.has(state.back)) state.back = "";
      // The trail keeps the steps the new bundle still has.
      setTrail(panelState.trail.filter((id) => nodes.has(id)));
      const view = views.get(state.view);
      if (view && has(view.sources, state.root)) open(state.view, state.root);
      else openHome();
    },
    destroy() {
      el.removeEventListener("click", onClick);
      el.removeEventListener("keydown", onKeyDown);
      resize?.disconnect();
      doc.removeEventListener("keydown", onDocumentKeyDown);
      doc.removeEventListener("click", onDocumentClick);
      for (const canvas of canvases.values()) canvas.mounted?.destroy();
      canvases.clear();
      panelListeners.clear();
      el.replaceChildren();
      el.classList.remove(ROOT_CLASS, "is-panel-closed");
      el.removeAttribute("aria-label");
      for (const name of Object.keys(brandVariables(brand, theme))) {
        el.style.removeProperty(name);
      }
      el.style.removeProperty("color-scheme");
    },
  };
}

function title(word: string) {
  return word.charAt(0).toUpperCase() + word.slice(1);
}

const SVG_NS = "http://www.w3.org/2000/svg";

function icon(doc: Document, paths: Array<[string, Record<string, string>]>) {
  const svg = doc.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  for (const [tag, attrs] of paths) {
    const child = doc.createElementNS(SVG_NS, tag);
    for (const [k, v] of Object.entries(attrs)) child.setAttribute(k, v);
    svg.append(child);
  }
  return svg;
}

function chevron(doc: Document) {
  return icon(doc, [["path", { d: "m6 9 6 6 6-6" }]]);
}

function plusIcon(doc: Document) {
  return icon(doc, [["path", { d: "M5 12h14M12 5v14" }]]);
}

function minusIcon(doc: Document) {
  return icon(doc, [["path", { d: "M5 12h14" }]]);
}

function fitIcon(doc: Document) {
  return icon(doc, [
    ["path", { d: "M8 3H5a2 2 0 0 0-2 2v3" }],
    ["path", { d: "M21 8V5a2 2 0 0 0-2-2h-3" }],
    ["path", { d: "M3 16v3a2 2 0 0 0 2 2h3" }],
    ["path", { d: "M16 21h3a2 2 0 0 0 2-2v-3" }],
  ]);
}

function panelIcon(doc: Document) {
  return icon(doc, [
    ["rect", { width: "18", height: "18", x: "3", y: "3", rx: "2" }],
    ["path", { d: "M15 3v18" }],
  ]);
}

function searchIcon(doc: Document) {
  return icon(doc, [
    ["circle", { cx: "11", cy: "11", r: "8" }],
    ["path", { d: "m21 21-4.3-4.3" }],
  ]);
}
