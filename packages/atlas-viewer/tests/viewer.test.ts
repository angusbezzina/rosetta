import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import fixture from "../demo/fixture-bundle.json" with { type: "json" };
import { brandVariables, DEFAULT_BRAND } from "../src/brand.ts";
import { readBundle } from "../src/read.ts";
import { GLOSSARY_CLASS, renderTerm } from "../src/term.ts";
import type { AtlasBundle } from "../src/types.ts";
import { type AtlasHandle, mount } from "../src/viewer.ts";

// happy-dom has no layout: Pele still renders (its text measurer falls back
// to estimates), so these tests see the SVG Pele emits, the classes and
// attributes the viewer puts on it and the panel state it publishes, but
// never geometry or the colours a browser would compute. The demo page is
// where those are checked. The panel's own drawing is tests/panel.test.tsx.

const bundle = readBundle(fixture);
let host: HTMLElement;
let atlas: AtlasHandle;

const q = <T extends Element = HTMLElement>(selector: string) =>
  host.querySelector(selector) as T | null;
const visibleCanvas = () =>
  [...host.querySelectorAll<HTMLElement>(".virgil-atlas-canvas")].find(
    (c) => !c.hidden,
  ) as HTMLElement;
const nodeEl = (id: string) =>
  visibleCanvas().querySelector(`[data-atlas-id="${id}"]`) as SVGGElement;
/** The label of the part the panel shows. */
const heading = () => {
  const { bundle: shown, trail } = atlas.panelState();
  const id = trail[trail.length - 1];
  return shown.nodes.find((n) => n.id === id)?.label;
};
const crumbs = () =>
  [
    ...host.querySelectorAll(
      ".virgil-atlas-crumbs > :not(.virgil-atlas-crumb-sep)",
    ),
  ].map((c) => c.textContent);
const key = (target: Element, init: KeyboardEventInit) =>
  target.dispatchEvent(
    new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }),
  );

beforeEach(() => {
  document.body.innerHTML = '<main id="host"></main>';
  host = document.getElementById("host") as HTMLElement;
  atlas = mount(host, { bundle });
});

afterEach(() => {
  atlas.destroy();
});

const click = (target: Element) => {
  const event = new MouseEvent("click", { bubbles: true, cancelable: true });
  (target.querySelector("a") ?? target).dispatchEvent(event);
  return event;
};
const crumb = (label: string) =>
  [
    ...host.querySelectorAll<HTMLButtonElement>(
      ".virgil-atlas-crumbs .virgil-atlas-crumb-link",
    ),
  ].find((b) => b.textContent === label) as HTMLButtonElement;
/** The fixture with changes, mounted in place of the default one. */
const remount = (change: (b: AtlasBundle) => void) => {
  atlas.destroy();
  const next = structuredClone(bundle);
  change(next);
  atlas = mount(host, { bundle: next });
};

describe("mount", () => {
  test("opens the overview with every node pressable and the first selected", () => {
    expect(q(".virgil-atlas-viewmenu-label")?.textContent).toBe("Overview");
    expect(crumbs()).toEqual(["virgil", "Overview"]);
    const nodes = visibleCanvas().querySelectorAll("[data-atlas-id]");
    expect(nodes.length).toBe(13);
    for (const node of nodes) {
      expect(node.getAttribute("role")).toBe("button");
      expect(node.getAttribute("tabindex")).toBe("0");
      expect(node.querySelector("a")?.getAttribute("tabindex")).toBe("-1");
    }
    expect(heading()).toBe("Desktop app");
    expect(q(".virgil-atlas-detail")?.getAttribute("aria-label")).toBe(
      "Desktop app",
    );
  });

  test("the view menu lists every view with a source at its default root", () => {
    const items = [...host.querySelectorAll(".virgil-atlas-viewmenu-item")].map(
      (i) => [
        i.querySelector(".virgil-atlas-viewmenu-name")?.textContent,
        i.querySelector(".virgil-atlas-viewmenu-desc")?.textContent,
      ],
    );
    expect(items).toEqual(
      bundle.views
        .filter((v) => v.kind !== "connections")
        .map((v) => [v.title, v.description]),
    );
    expect(host.querySelector('[data-view="connections"]')).toBeNull();
    expect(() => atlas.open("connections")).toThrow(
      'atlas: view "connections" has no source for ""',
    );
    const button = q<HTMLButtonElement>(".virgil-atlas-viewmenu-btn");
    button?.click();
    expect(q(".virgil-atlas-viewmenu-list")?.hidden).toBe(false);
    (host.querySelector('[data-view="infra"]') as HTMLElement).click();
    expect(q(".virgil-atlas-viewmenu-list")?.hidden).toBe(true);
    expect(q(".virgil-atlas-viewmenu-label")?.textContent).toBe("Infra");
    expect(crumbs()).toEqual(["virgil", "Infra"]);
  });

  test("the toolbar is the breadcrumb, the view menu and the panel toggle, no Find chip", () => {
    expect(
      [...(q(".virgil-atlas-toolbar-right")?.children ?? [])].map(
        (c) => c.className,
      ),
    ).toEqual(["virgil-atlas-viewmenu", "virgil-atlas-panel-toggle"]);
    expect(host.querySelector('[data-action="find-open"]')).toBeNull();
  });
});

describe("the panel's element", () => {
  const toggle = () =>
    q<HTMLButtonElement>(".virgil-atlas-panel-toggle") as HTMLButtonElement;

  afterEach(() => localStorage.clear());

  test("the viewer leaves it empty and publishes what it shows", () => {
    expect(q(".virgil-atlas-detail")).toBe(atlas.panel);
    expect(atlas.panel.childElementCount).toBe(0);
    expect(atlas.panelState()).toEqual({ bundle, trail: ["desktop-app"] });
    const seen: string[][] = [];
    const stop = atlas.onPanelChange(() =>
      seen.push([...atlas.panelState().trail]),
    );
    const before = atlas.panelState();
    atlas.select("door");
    expect(atlas.panelState()).not.toBe(before);
    atlas.drill("d:crates/virgil-serve/src");
    atlas.drill("door");
    stop();
    atlas.select("hud");
    expect(seen).toEqual([
      ["door"],
      ["door", "d:crates/virgil-serve/src"],
      ["door"],
    ]);
    expect(atlas.panel.getAttribute("aria-label")).toBe("HUD");
    expect(atlas.panel.childElementCount).toBe(0);
  });

  test("a drill into a part the canvas draws selects it there too", () => {
    atlas.select("door");
    atlas.drill("hud");
    expect(atlas.panelState().trail).toEqual(["door", "hud"]);
    expect(nodeEl("hud").classList.contains("is-selected")).toBe(true);
    atlas.drill("d:crates/virgil-serve/src");
    // Drawn nowhere, so the canvas keeps the HUD.
    expect(nodeEl("hud").classList.contains("is-selected")).toBe(true);
  });

  test("the panel closes from the host, the toolbar button only opens it, and both are remembered", () => {
    // While the panel is open, nothing in the toolbar opens or closes it.
    expect(toggle().hidden).toBe(true);
    expect(toggle().getAttribute("aria-controls")).toBe(atlas.panel.id);
    expect(toggle().getAttribute("aria-label")).toBe("Open detail panel");
    expect(toggle().title).toBe("Open detail panel");
    atlas.setPanelOpen(false);
    expect(atlas.panel.hidden).toBe(true);
    expect(toggle().hidden).toBe(false);
    expect(host.classList.contains("is-panel-closed")).toBe(true);
    expect(localStorage.getItem("virgil-atlas:panel")).toBe("closed");

    atlas.destroy();
    atlas = mount(host, { bundle });
    expect(atlas.panel.hidden).toBe(true);
    toggle().click();
    expect(atlas.panel.hidden).toBe(false);
    expect(toggle().hidden).toBe(true);
    expect(host.classList.contains("is-panel-closed")).toBe(false);
    expect(localStorage.getItem("virgil-atlas:panel")).toBe("open");
  });

  test("a narrow first visit gives the diagram full width but honours a later panel choice", () => {
    const original = Object.getOwnPropertyDescriptor(window, "innerWidth");
    const saved = localStorage.getItem("virgil-atlas:panel");
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 900,
    });
    localStorage.removeItem("virgil-atlas:panel");
    try {
      atlas.destroy();
      atlas = mount(host, { bundle });
      expect(atlas.panel.hidden).toBe(true);
      expect(host.classList.contains("is-panel-closed")).toBe(true);
      atlas.setPanelOpen(true);
      atlas.destroy();
      atlas = mount(host, { bundle });
      expect(atlas.panel.hidden).toBe(false);
    } finally {
      if (original) Object.defineProperty(window, "innerWidth", original);
      else Reflect.deleteProperty(window, "innerWidth");
      if (saved === null) localStorage.removeItem("virgil-atlas:panel");
      else localStorage.setItem("virgil-atlas:panel", saved);
    }
  });

  test("closing the panel from inside it hands focus to the button that reopens it", () => {
    const inside = document.createElement("button");
    atlas.panel.append(inside);
    inside.focus();
    atlas.setPanelOpen(false);
    expect(document.activeElement).toBe(toggle());
    inside.remove();
  });

  test("storage that refuses leaves the panel open, closing and reopening", () => {
    const original = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("blocked");
      },
    });
    try {
      atlas.destroy();
      atlas = mount(host, { bundle });
      expect(atlas.panel.hidden).toBe(false);
      atlas.setPanelOpen(false);
      expect(atlas.panel.hidden).toBe(true);
      toggle().click();
      expect(atlas.panel.hidden).toBe(false);
    } finally {
      if (original) Object.defineProperty(window, "localStorage", original);
    }
  });
});

describe("clicking", () => {
  test("selection follows incoming and outgoing rendered edges without losing the rest of the map", () => {
    atlas.select("daemon");
    const edge = (id: string) => host.querySelector<SVGGElement>(`.virgil-atlas-canvas:not([hidden]) .pele-edge[data-id="${id}"]`);
    const label = (id: string) => host.querySelector<SVGGElement>(`.virgil-atlas-canvas:not([hidden]) .pele-edge-label[data-id="${id}"]`);
    expect(edge("L_n1_n3_0")?.classList.contains("is-connection-in")).toBe(true);
    expect(edge("L_n3_n5_0")?.classList.contains("is-connection-out")).toBe(true);
    expect(edge("L_n6_n8_0")?.classList.contains("is-connection-muted")).toBe(true);
    expect(label("L_n1_n3_0")?.classList.contains("is-connection-in")).toBe(true);
    expect(nodeEl("door").classList.contains("is-connection-neighbour")).toBe(true);
  });
  test("a click on a connected node selects it and opens its Connections", () => {
    const door = nodeEl("door");
    expect(click(door).defaultPrevented).toBe(true);
    expect(crumbs()).toEqual(["virgil", "Overview", "Door"]);
    expect(
      q(".virgil-atlas-crumbs .virgil-atlas-crumb-here")?.getAttribute(
        "aria-current",
      ),
    ).toBe("location");
    // Named for the part it is about.
    expect(q(".virgil-atlas-viewmenu-label")?.textContent).toBe(
      "Door Connections",
    );
    const focus = nodeEl("door");
    expect(focus.classList.contains("is-focus")).toBe(true);
    expect(focus.classList.contains("is-selected")).toBe(true);
    expect(focus.getAttribute("aria-pressed")).toBe("true");
    expect(
      [...visibleCanvas().querySelectorAll("[data-atlas-id]")].map((n) =>
        n.getAttribute("data-atlas-id"),
      ),
    ).toEqual([
      "daemon",
      "cato",
      "door",
      "postgres-cells",
      "cloud-kms",
      "realtime-edge",
    ]);
    expect(heading()).toBe("Door");
  });

  test("Enter and Space on a focused node do what a click does", () => {
    key(nodeEl("cato"), { key: "Enter" });
    expect(heading()).toBe("Cato");
    expect(crumbs()).toEqual(["virgil", "Overview", "Cato"]);
    key(nodeEl("door"), { key: " " });
    expect(crumbs()).toEqual(["virgil", "Overview", "Door"]);
  });

  test("a click on a node with no Connections only selects it", () => {
    remount((b) => {
      const connections = b.views.find((v) => v.kind === "connections");
      if (connections) delete connections.sources.hud;
    });
    click(nodeEl("hud"));
    expect(crumbs()).toEqual(["virgil", "Overview"]);
    expect(heading()).toBe("HUD");
    expect(nodeEl("hud").classList.contains("is-selected")).toBe(true);
  });

  test("a click on the focus node, or a neighbour, walks the Connections", () => {
    click(nodeEl("door"));
    click(nodeEl("door"));
    expect(crumbs()).toEqual(["virgil", "Overview", "Door"]);
    click(nodeEl("daemon"));
    expect(crumbs()).toEqual(["virgil", "Overview", "Daemon"]);
    expect(nodeEl("daemon").classList.contains("is-focus")).toBe(true);
  });

  test("the return crumb goes back and keeps the selection", () => {
    atlas.open("infra");
    click(nodeEl("run-door"));
    expect(crumbs()).toEqual(["virgil", "Infra", "Door on Cloud Run"]);
    crumb("Infra").click();
    expect(crumbs()).toEqual(["virgil", "Infra"]);
    expect(nodeEl("run-door").classList.contains("is-selected")).toBe(true);
    click(nodeEl("cloud-sql"));
    crumb("virgil").click();
    expect(crumbs()).toEqual(["virgil", "Overview"]);
    click(nodeEl("door"));
    crumb("Overview").click();
    expect(heading()).toBe("Door");
    expect(nodeEl("door").classList.contains("is-selected")).toBe(true);
  });

  test("Records opens on its areas; an area opens its collections", () => {
    atlas.open("records");
    expect(crumbs()).toEqual(["virgil", "Records"]);
    click(nodeEl("records-area:tasks"));
    expect(crumbs()).toEqual(["virgil", "Records", "Tasks and goals"]);
    expect(heading()).toBe("Tasks and goals");
    const task = nodeEl("records:task");
    expect(task.classList.contains("scope-private")).toBe(true);
    expect(nodeEl("records:goal").classList.contains("scope-recipient")).toBe(
      true,
    );
    click(task);
    expect(crumbs()).toEqual(["virgil", "Records", "task"]);
    crumb("Records").click();
    expect(crumbs()).toEqual(["virgil", "Records"]);
    // The area a ref crosses to is drawn in the area's picture, and opens.
    click(nodeEl("records-area:tasks"));
    click(nodeEl("records-area:messages"));
    expect(crumbs()).toEqual(["virgil", "Records", "Messages and channels"]);
    expect(heading()).toBe("Messages and channels");
  });

  test("a flow draws each part once, inside its tier", () => {
    atlas.open("send-a-message");
    expect(crumbs()).toEqual(["virgil", "Flow"]);
    atlas.select("daemon");
    expect(
      visibleCanvas().querySelectorAll('[data-atlas-id="daemon"]').length,
    ).toBe(1);
    expect(visibleCanvas().querySelectorAll("[data-atlas-id]").length).toBe(4);
    expect(visibleCanvas().textContent).toContain(
      "4. Carries the signed bundle",
    );
  });

  test("select() selects without a click", () => {
    atlas.select("realtime-edge");
    expect(nodeEl("realtime-edge").classList.contains("is-selected")).toBe(
      true,
    );
    expect(heading()).toBe("Realtime edge");
    expect(crumbs()).toEqual(["virgil", "Overview"]);
  });

  test("no double-click: a second click is a click", () => {
    nodeEl("door").dispatchEvent(
      new MouseEvent("dblclick", { bubbles: true, cancelable: true }),
    );
    expect(crumbs()).toEqual(["virgil", "Overview"]);
    expect(heading()).toBe("Desktop app");
  });
});

describe("subgraphs and overflow", () => {
  test("a subgraph is never selectable, focusable or a find match", () => {
    const clusters = [...visibleCanvas().querySelectorAll(".pele-cluster")];
    expect(clusters.map((c) => c.textContent).sort()).toEqual([
      "Cell",
      "Device",
      "Edge",
      "External",
      "Providers",
    ]);
    for (const cluster of clusters) {
      expect(cluster.hasAttribute("tabindex")).toBe(false);
      expect(cluster.hasAttribute("role")).toBe(false);
      expect(cluster.closest("[data-atlas-id]")).toBeNull();
    }
    click(clusters[0] as Element);
    expect(heading()).toBe("Desktop app");
    expect(crumbs()).toEqual(["virgil", "Overview"]);
    atlas.find("Device");
    expect(q(".virgil-atlas-find-count")?.textContent).toBe("No matches");
  });

  test("a +N more node is drawn but not pressable", () => {
    remount((b) => {
      const connections = b.views.find((v) => v.kind === "connections");
      if (connections) {
        connections.sources.door = `flowchart LR
  n1["Door"]
  n2["Daemon"]
  n3["+3 more"]
  n2 -->|calls| n1
  n3 -.-> n1
  class n1 fam-service,kind-service,is-focus
  class n2 fam-service,kind-service
  class n3 is-more
  click n1 "atlas:door"
  click n2 "atlas:daemon"
`;
      }
    });
    click(nodeEl("door"));
    expect(visibleCanvas().textContent).toContain("+3 more");
    expect(
      [...visibleCanvas().querySelectorAll("[data-atlas-id]")].map((n) =>
        n.getAttribute("data-atlas-id"),
      ),
    ).toEqual(["door", "daemon"]);
  });
});

describe("views", () => {
  test("a hidden view keeps its SVG and a repeat open renders nothing new", () => {
    atlas.open("infra");
    const svg = visibleCanvas().querySelector("svg");
    atlas.open("overview");
    atlas.open("infra");
    expect(visibleCanvas().querySelector("svg")).toBe(svg);
    expect(host.querySelectorAll(".virgil-atlas-canvas").length).toBe(2);
  });

  test("a left-to-right chart stays a row in a narrow pane", () => {
    // Pele measures the container when mounted; a width under its 640px
    // breakpoint is where it used to turn a row into a column.
    const width = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "clientWidth",
    );
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get: () => 300,
    });
    try {
      atlas.open("send-a-message");
      const svg = visibleCanvas().querySelector("svg") as SVGSVGElement;
      const [, , w, h] = (svg.getAttribute("viewBox") ?? "")
        .split(" ")
        .map(Number);
      expect(w).toBeGreaterThan(300);
      expect(w).toBeGreaterThan(h ?? Number.POSITIVE_INFINITY);
    } finally {
      if (width) {
        Object.defineProperty(HTMLElement.prototype, "clientWidth", width);
      }
    }
  });

  test("Data draws each store as a cylinder", () => {
    atlas.open("data");
    for (const id of ["sqlite-replica", "postgres-cells", "cloud-kms"]) {
      expect(nodeEl(id).classList.contains("pele-shape-cyl")).toBe(true);
    }
  });

  test("a new bundle re-renders only the views whose source changed", () => {
    // Pele defers an update while its element has no width (hidden, or here
    // unlaid-out) and renders once a ResizeObserver reports one; give the
    // elements a width so the update happens now.
    const width = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "clientWidth",
    );
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get: () => 800,
    });
    try {
      const overviewSvg = visibleCanvas().querySelector("svg");
      atlas.open("infra");
      const infraSvg = visibleCanvas().querySelector("svg");
      const next = structuredClone(bundle);
      const infra = next.views.find((v) => v.id === "infra");
      if (infra) {
        infra.sources[""] = (infra.sources[""] ?? "").replace(
          "Cloud SQL",
          "Cloud SQL cells",
        );
      }
      atlas.setBundle(next);
      expect(visibleCanvas().querySelector("svg")).not.toBe(infraSvg);
      expect(visibleCanvas().textContent).toContain("Cloud SQL cells");
      atlas.open("overview");
      expect(visibleCanvas().querySelector("svg")).toBe(overviewSvg);
    } finally {
      if (width) {
        Object.defineProperty(HTMLElement.prototype, "clientWidth", width);
      }
    }
  });
});

describe("pan and zoom", () => {
  const rect = (left: number, top: number, width: number, height: number) =>
    ({
      left,
      top,
      width,
      height,
      right: left + width,
      bottom: top + height,
      x: left,
      y: top,
      toJSON: () => ({}),
    }) as DOMRect;
  const figure = () => q(".virgil-atlas-figure") as HTMLElement;
  /** happy-dom lays nothing out: give the figure a size, then reopen. */
  const size = (width: number, height: number) => {
    const f = figure();
    Object.defineProperty(f, "clientWidth", {
      configurable: true,
      value: width,
    });
    Object.defineProperty(f, "clientHeight", {
      configurable: true,
      value: height,
    });
    f.getBoundingClientRect = () => rect(0, 0, width, height);
  };
  const transform = () => {
    const m =
      /^translate\((-?[\d.e+-]+)px, (-?[\d.e+-]+)px\) scale\(([\d.e+-]+)\)$/.exec(
        visibleCanvas().style.transform,
      );
    if (!m)
      throw new Error(`no transform: "${visibleCanvas().style.transform}"`);
    return { x: Number(m[1]), y: Number(m[2]), scale: Number(m[3]) };
  };
  const natural = () => {
    const svg = visibleCanvas().querySelector("svg") as SVGSVGElement;
    return {
      width: Number(svg.getAttribute("width")),
      height: Number(svg.getAttribute("height")),
    };
  };
  const pointer = (
    target: Element,
    type: string,
    clientX: number,
    clientY: number,
  ) =>
    target.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        cancelable: true,
        pointerId: 1,
        button: 0,
        clientX,
        clientY,
      }),
    );
  const wheel = (
    deltaY: number,
    clientX: number,
    clientY: number,
    ctrlKey = false,
  ) => {
    const event = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaY,
      ctrlKey,
    });
    // happy-dom's WheelEvent drops the mouse coordinates it is given.
    Object.defineProperties(event, {
      clientX: { value: clientX },
      clientY: { value: clientY },
    });
    figure().dispatchEvent(event);
  };

  test("the figure never scrolls and the canvas is moved by its transform", () => {
    const style = document.createElement("style");
    style.textContent = readFileSync(
      join(import.meta.dir, "../src/styles.css"),
      "utf8",
    );
    document.head.append(style);
    try {
      expect(getComputedStyle(figure()).overflow).toBe("hidden");
      const canvas = getComputedStyle(visibleCanvas());
      expect(canvas.position).toBe("absolute");
      expect(canvas.transformOrigin).toBe("0 0");
      // One layout pixel to one CSS pixel: Pele's own sizing is untouched.
      const svg = visibleCanvas().querySelector("svg") as SVGSVGElement;
      const [, , w] = (svg.getAttribute("viewBox") ?? "")
        .split(" ")
        .map(Number);
      expect(Number(svg.getAttribute("width"))).toBe(w as number);
      expect(svg.style.maxWidth).toBe("");
    } finally {
      style.remove();
    }
  });

  test("a diagram that fits opens at its own size, centred", () => {
    size(4000, 3000);
    atlas.open("overview");
    const { width, height } = natural();
    expect(transform()).toEqual({
      x: (4000 - width) / 2,
      y: (3000 - height) / 2,
      scale: 1,
    });
  });

  test("a larger one opens fitted, the whole diagram in the figure", () => {
    atlas.open("overview");
    const { width, height } = natural();
    // Room for it at 0.8: fitted, centred.
    size(width * 0.8 + 32, height * 0.8 + 32);
    atlas.open("infra");
    atlas.open("overview");
    expect(transform().scale).toBeCloseTo(0.8, 6);
    // In a narrow pane, a tiny fit makes the labels unreadable. Open at
    // natural size, show the navigation hint, and leave Fit available.
    size(width * 0.3 + 32, height * 0.3 + 32);
    atlas.open("infra");
    atlas.open("overview");
    expect(transform().scale).toBe(1);
    expect(transform().x).toBeCloseTo(16, 6);
    expect(transform().y).toBeCloseTo(16, 6);
    expect(q<HTMLElement>(".virgil-atlas-pan-hint")?.hidden).toBe(false);
    // Fitted is as far out as it goes, after an explicit Fit command.
    q<HTMLButtonElement>('.virgil-atlas-pan-hint [data-action="zoom-fit"]')?.click();
    expect(transform().scale).toBeCloseTo(0.3, 6);
    expect(
      q<HTMLButtonElement>('.virgil-atlas-zoom [data-action="zoom-out"]')
        ?.disabled,
    ).toBe(true);
    for (let i = 0; i < 10; i++) wheel(100, 10, 10);
    expect(transform().scale).toBeCloseTo(0.3, 6);
  });

  test("a focused Connections view opens with both sides visible at a legible fit", () => {
    const connections = bundle.views.find((view) => view.kind === "connections");
    const root = Object.keys(connections?.sources ?? {})[0];
    expect(root).toBeDefined();
    atlas.open("connections", root);
    const { width, height } = natural();
    size(width * 0.75 + 32, height * 0.75 + 32);
    atlas.open("overview");
    atlas.open("connections", root);
    expect(transform().scale).toBeCloseTo(0.75, 6);
    expect(transform().x).toBeCloseTo(16, 6);
    expect(transform().y).toBeCloseTo(16, 6);
    expect(q<HTMLElement>(".virgil-atlas-pan-hint")?.hidden).toBe(true);
  });

  test("closing the panel re-fits a diagram the reader has not moved", () => {
    atlas.open("overview");
    const { width, height } = natural();
    size(width * 0.5 + 32, height * 0.5 + 32);
    atlas.open("infra");
    atlas.open("overview");
    expect(transform().scale).toBeCloseTo(0.85, 6);
    expect(q<HTMLElement>(".virgil-atlas-pan-hint")?.hidden).toBe(false);
    // The figure gains the panel's width; its observer places it afresh.
    size(width * 0.8 + 32, height * 0.8 + 32);
    atlas.setPanelOpen(false);
    atlas.open("overview");
    expect(transform().scale).toBeCloseTo(0.8, 6);
    localStorage.clear();
  });

  test("the wheel zooms about the cursor", () => {
    size(4000, 3000);
    atlas.open("overview");
    const before = transform();
    const at = { x: before.x + 120, y: before.y + 40 };
    const under = (t: typeof before) => ({
      x: (at.x - t.x) / t.scale,
      y: (at.y - t.y) / t.scale,
    });
    wheel(-50, at.x, at.y);
    const after = transform();
    expect(after.scale).toBeGreaterThan(before.scale);
    expect(under(after).x).toBeCloseTo(under(before).x, 6);
    expect(under(after).y).toBeCloseTo(under(before).y, 6);
    // A pinch is a ctrl wheel, anchored the same way.
    wheel(8, at.x, at.y, true);
    const pinched = transform();
    expect(pinched.scale).toBeLessThan(after.scale);
    expect(under(pinched).x).toBeCloseTo(under(before).x, 6);
    // Clamped: never past four times.
    for (let i = 0; i < 40; i++) wheel(-100, at.x, at.y);
    expect(transform().scale).toBe(4);
  });

  test("a drag pans and never clicks; a press without movement still does", () => {
    size(4000, 3000);
    atlas.open("overview");
    const door = nodeEl("door");
    const before = transform();
    pointer(door, "pointerdown", 100, 100);
    pointer(door, "pointermove", 103, 101);
    expect(transform()).toEqual(before);
    pointer(door, "pointermove", 160, 130);
    expect(figure().classList.contains("is-panning")).toBe(true);
    pointer(door, "pointerup", 160, 130);
    const swallowed = click(door);
    expect(swallowed.defaultPrevented).toBe(true);
    expect(transform()).toEqual({
      x: before.x + 60,
      y: before.y + 30,
      scale: 1,
    });
    expect(figure().classList.contains("is-panning")).toBe(false);
    expect(crumbs()).toEqual(["virgil", "Overview"]);
    expect(heading()).toBe("Desktop app");

    pointer(door, "pointerdown", 100, 100);
    pointer(door, "pointermove", 102, 102);
    pointer(door, "pointerup", 102, 102);
    click(door);
    expect(heading()).toBe("Door");
    expect(crumbs()).toEqual(["virgil", "Overview", "Door"]);
  });

  test("the controls and + - 0 zoom by steps and fit; a new view starts placed", () => {
    size(4000, 3000);
    atlas.open("overview");
    const button = (action: string) =>
      q<HTMLButtonElement>(
        `.virgil-atlas-zoom [data-action="${action}"]`,
      ) as HTMLButtonElement;
    button("zoom-in").click();
    expect(transform().scale).toBeCloseTo(1.25, 6);
    key(figure(), { key: "+" });
    expect(transform().scale).toBeCloseTo(1.5625, 6);
    key(nodeEl("door"), { key: "-" });
    expect(transform().scale).toBeCloseTo(1.25, 6);
    button("zoom-out").click();
    expect(transform().scale).toBeCloseTo(1, 6);
    button("zoom-in").click();
    key(figure(), { key: "0" });
    const { width } = natural();
    expect(transform()).toMatchObject({ x: (4000 - width) / 2, scale: 1 });
    // A pan is reset by switching views.
    pointer(figure(), "pointerdown", 10, 10);
    pointer(figure(), "pointermove", 200, 200);
    pointer(figure(), "pointerup", 200, 200);
    atlas.open("infra");
    atlas.open("overview");
    expect(transform()).toMatchObject({ x: (4000 - width) / 2, scale: 1 });
    expect(figure().getAttribute("tabindex")).toBe("0");
  });

  test("a narrow off-screen selection leaves room for incoming labels; find centres", () => {
    size(600, 400);
    atlas.open("overview");
    const start = transform();
    nodeEl("door").getBoundingClientRect = () => rect(650, 100, 80, 40);
    nodeEl("hud").getBoundingClientRect = () => rect(100, 100, 80, 40);
    atlas.select("door");
    // A fitted diagram sits at fractional pixels, so the sums are near.
    expect(transform().x).toBeCloseTo(start.x + 450 - 690, 6);
    expect(transform().y).toBeCloseTo(start.y + 200 - 120, 6);
    expect(visibleCanvas().classList.contains("is-gliding")).toBe(true);
    const moved = transform();
    atlas.select("hud");
    expect(transform()).toEqual(moved);
    atlas.find("hud");
    expect(transform().x).toBeCloseTo(moved.x + 300 - 140, 6);
  });

  test("Pele's own zoom stays off", () => {
    expect(q(".pele-zoom")).toBeNull();
  });
});

describe("the view menu", () => {
  test("its list is bounded by the space below its trigger and scrolls", () => {
    const style = document.createElement("style");
    style.textContent = readFileSync(
      join(import.meta.dir, "../src/styles.css"),
      "utf8",
    );
    document.head.append(style);
    try {
      host.getBoundingClientRect = () =>
        ({
          top: 0,
          bottom: 500,
          height: 500,
          left: 0,
          right: 900,
          width: 900,
        }) as DOMRect;
      (q(".virgil-atlas-viewmenu-btn") as HTMLElement).getBoundingClientRect =
        () =>
          ({
            top: 12,
            bottom: 44,
            height: 32,
            left: 700,
            right: 880,
            width: 180,
          }) as DOMRect;
      q<HTMLButtonElement>(".virgil-atlas-viewmenu-btn")?.click();
      const list = q(".virgil-atlas-viewmenu-list") as HTMLElement;
      expect(list.style.maxHeight).toBe(`${500 - 44 - 6 - 12}px`);
      expect(getComputedStyle(list).overflowY).toBe("auto");
    } finally {
      style.remove();
    }
  });
});

describe("narrow connections", () => {
  test("shows every typed relationship as readable flow, including reversed reads", () => {
    const figure = q<HTMLElement>(".virgil-atlas-figure")!;
    Object.defineProperty(figure, "clientWidth", { configurable: true, value: 390 });
    atlas.open("connections", "door");
    expect(figure.classList.contains("is-connection-list")).toBe(true);
    const list = q<HTMLElement>(".virgil-atlas-connection-list")!;
    expect(list.querySelectorAll("li").length).toBe(
      bundle.edges.filter((edge) => edge.from === "door" || edge.to === "door").length,
    );
    const read = [...list.querySelectorAll("li")]
      .find((item) => item.textContent?.includes("Door reads Cloud KMS"));
    expect(read).toBeDefined();
    expect([...read!.querySelectorAll(".virgil-atlas-connection-part")].map((part) => part.textContent))
      .toEqual(["Cloud KMS", "Door"]);
    expect(read?.textContent).toContain("atlas model");
    const wheel = new WheelEvent("wheel", { deltaY: 100, bubbles: true, cancelable: true });
    figure.dispatchEvent(wheel);
    expect(wheel.defaultPrevented).toBe(false);

    const mode = q<HTMLButtonElement>(".virgil-atlas-mode-btn")!;
    expect(mode.textContent).toBe("Diagram");
    mode.click();
    expect(figure.classList.contains("is-connection-list")).toBe(false);
    expect(mode.textContent).toBe("Readable list");
    mode.click();
    (read!.querySelector(".virgil-atlas-connection-part") as HTMLButtonElement).click();
    expect(crumbs()).toEqual(["virgil", "Overview", "Cloud KMS"]);
    expect(figure.classList.contains("is-connection-list")).toBe(true);
  });

  test("a focused capability keeps its relationships and implementation readable", () => {
    remount((b) => {
      b.views = [{
        id: "focused-capability",
        kind: "custom",
        title: "Request path",
        description: "Focused component view",
        default_root: "",
        sources: { "": b.views[0]!.sources[""]! },
      }];
      b.focused_view_id = "focused-capability";
      b.capabilities = [{
        id: "request-path",
        name: "Request path",
        purpose: "Answer a request",
        status: "proposed",
        terms: [],
        parts: ["door", "daemon"],
        implementation: [{ part: "door", path: "crates/virgil-serve/src/main.rs" }],
      }];
      b.target_exists = { "crates/virgil-serve/src/main.rs": true };
    });
    const figure = q<HTMLElement>(".virgil-atlas-figure")!;
    Object.defineProperty(figure, "clientWidth", { configurable: true, value: 390 });
    atlas.open("focused-capability");
    atlas.select("door");
    expect(figure.classList.contains("is-connection-list")).toBe(true);
    const list = q<HTMLElement>(".virgil-atlas-connection-list")!;
    expect(list.textContent).toContain("Door reads Cloud KMS");
    expect(list.textContent).toContain("Implementation (1)");
    expect(list.textContent).toContain("crates/virgil-serve/src/main.rs");
    expect(list.textContent).toContain("Path exists in this snapshot");
    atlas.select("daemon");
    expect(list.textContent).toContain("Daemon ·");
    expect(list.textContent).not.toContain("Implementation (1)");
  });
});

describe("find", () => {
  test("Cmd+F opens the bar, filters, cycles and Esc closes", () => {
    const event = new KeyboardEvent("keydown", {
      key: "f",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    const bar = q(".virgil-atlas-find") as HTMLElement;
    const input = q<HTMLInputElement>(
      ".virgil-atlas-find-input",
    ) as HTMLInputElement;
    expect(bar.hidden).toBe(false);
    expect(document.activeElement).toBe(input);

    input.value = "virgil-serve";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    // label, path and term name: "virgil-serve" is the path of door, worker and delivery
    const matches = () =>
      [...visibleCanvas().querySelectorAll(".is-match")].map((n) =>
        n.getAttribute("data-atlas-id"),
      );
    expect(matches()).toEqual(["door", "worker", "delivery"]);
    expect(visibleCanvas().classList.contains("is-finding")).toBe(true);
    expect(q(".virgil-atlas-find-count")?.textContent).toBe("1 of 3");
    expect(heading()).toBe("Door");

    key(input, { key: "Enter" });
    expect(q(".virgil-atlas-find-count")?.textContent).toBe("2 of 3");
    expect(heading()).toBe("Worker");
    key(input, { key: "Enter", shiftKey: true });
    key(input, { key: "Enter", shiftKey: true });
    expect(q(".virgil-atlas-find-count")?.textContent).toBe("3 of 3");
    expect(heading()).toBe("Delivery");

    input.value = "edge";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    expect(matches()).toEqual(["realtime-edge"]);

    key(input, { key: "Escape" });
    expect(bar.hidden).toBe(true);
    expect(matches()).toEqual([]);
    expect(visibleCanvas().classList.contains("is-finding")).toBe(false);
    // Focus lands on what find left selected.
    expect(document.activeElement).toBe(nodeEl("realtime-edge"));
  });

  test("the find bar's close button closes it onto the selection too", () => {
    atlas.find("Cato");
    (q(".virgil-atlas-find-close") as HTMLButtonElement).click();
    expect((q(".virgil-atlas-find") as HTMLElement).hidden).toBe(true);
    expect(document.activeElement).toBe(nodeEl("cato"));
  });

  test("find selects without opening Connections", () => {
    atlas.find("Door");
    expect(heading()).toBe("Door");
    expect(crumbs()).toEqual(["virgil", "Overview"]);
  });

  test("find() matches term names too, and No matches says so", () => {
    atlas.find("Daemon");
    expect(q(".virgil-atlas-find-count")?.textContent).toBe("1 of 1");
    expect(heading()).toBe("Daemon");
    atlas.find("zzz");
    expect(q(".virgil-atlas-find-count")?.textContent).toBe("No matches");
  });

  test("find reaches capability terms and their recorded Avoid words", () => {
    remount((next) => {
      next.capabilities = [{
        id: "request-path",
        name: "Request path",
        purpose: "Answer a request",
        status: "proposed",
        terms: ["term.door"],
        parts: ["daemon", "door"],
        implementation: [],
      }];
    });
    atlas.find("backend");
    const matches = [...visibleCanvas().querySelectorAll(".is-match")]
      .map((node) => node.getAttribute("data-atlas-id"));
    expect(matches).toEqual(["daemon", "door"]);
    expect(q(".virgil-atlas-find-count")?.textContent).toBe("1 of 2");
  });

  test("without captureFind no key opens find; the host calls find()", () => {
    atlas.destroy();
    const listening = document.addEventListener;
    const added: string[] = [];
    document.addEventListener = ((type: string, ...rest: unknown[]) => {
      added.push(type);
      return (listening as (...args: unknown[]) => void).call(
        document,
        type,
        ...rest,
      );
    }) as typeof document.addEventListener;
    try {
      atlas = mount(host, { bundle, captureFind: false });
    } finally {
      document.addEventListener = listening;
    }
    expect(added).not.toContain("keydown");

    for (const target of [document.body, nodeEl("door")]) {
      if (target !== document.body) (target as SVGGElement).focus();
      const press = new KeyboardEvent("keydown", {
        key: "f",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      });
      target.dispatchEvent(press);
      expect(press.defaultPrevented).toBe(false);
      expect((q(".virgil-atlas-find") as HTMLElement).hidden).toBe(true);
    }

    atlas.find();
    expect((q(".virgil-atlas-find") as HTMLElement).hidden).toBe(false);
  });

  test("find() with no query opens the bar on the query it had", () => {
    atlas.find("virgil-serve");
    key(q(".virgil-atlas-find-input") as HTMLElement, { key: "Escape" });
    atlas.find();
    const input = q<HTMLInputElement>(".virgil-atlas-find-input");
    expect(input?.value).toBe("virgil-serve");
    expect(q(".virgil-atlas-find-count")?.textContent).toBe("1 of 3");
  });
});

describe("colours", () => {
  test("a theme swap changes no DOM, only the variables on the mount", () => {
    const before = host.innerHTML;
    const tint = host.style.getPropertyValue("--atlas-tint");
    expect(host.style.getPropertyValue("color-scheme")).toBe("light");
    atlas.setTheme("dark");
    expect(host.innerHTML).toBe(before);
    expect(host.style.getPropertyValue("--atlas-tint")).not.toBe(tint);
    // Native scrollbars and controls follow the theme the host gave.
    expect(host.style.getPropertyValue("color-scheme")).toBe("dark");
  });

  test("the brand colours the families; defaults are neutral", () => {
    expect(host.style.getPropertyValue("--atlas-surface")).toBe(
      DEFAULT_BRAND.primary,
    );
    expect(host.style.getPropertyValue("--pele-accent")).toBe(
      DEFAULT_BRAND.primary,
    );
    const before = host.innerHTML;
    atlas.setBrand({ primary: "#123456", tertiary: "not a colour" });
    expect(host.innerHTML).toBe(before);
    expect(host.style.getPropertyValue("--atlas-surface")).toBe("#123456");
    expect(host.style.getPropertyValue("--atlas-store")).toBe(
      DEFAULT_BRAND.tertiary,
    );
    expect(brandVariables(DEFAULT_BRAND, "light")["--atlas-agent"]).toBe(
      "var(--rosetta-agent, hsl(var(--muted-foreground)))",
    );
  });

  test("a changed primary changes the surface family's computed stroke", () => {
    const style = document.createElement("style");
    style.textContent = readFileSync(
      join(import.meta.dir, "../src/styles.css"),
      "utf8",
    );
    document.head.append(style);
    try {
      const shape = nodeEl("hud").querySelector("rect") as Element;
      const service = nodeEl("door").querySelector("rect") as Element;
      expect(getComputedStyle(shape).stroke).toBe(DEFAULT_BRAND.primary);
      atlas.setBrand({ primary: "#123456" });
      expect(getComputedStyle(shape).stroke).toBe("#123456");
      expect(getComputedStyle(service).stroke).toBe(DEFAULT_BRAND.secondary);
    } finally {
      style.remove();
    }
  });

  test("no hex colour in src", () => {
    const src = join(import.meta.dir, "../src");
    const offenders = readdirSync(src, { recursive: true, encoding: "utf8" })
      .filter((file) => /\.(?:ts|tsx|css)$/.test(file) && file !== "brand.ts")
      .filter((file) =>
        /#[0-9a-fA-F]{3,8}\b/.test(readFileSync(join(src, file), "utf8")),
      );
    expect(offenders).toEqual([]);
  });
});

test("destroy leaves the host as it found it", () => {
  atlas.destroy();
  expect(host.childElementCount).toBe(0);
  expect(host.classList.contains("virgil-atlas")).toBe(false);
  expect(host.getAttribute("style") ?? "").toBe("");
  atlas = mount(host, { bundle });
});

test("readBundle says where a file is not a bundle", () => {
  expect(() => readBundle({ schema: "virgil.atlas/v1" })).toThrow(
    "atlas bundle: schema: expected rosetta.atlas-bundle/v1 or virgil.atlas-bundle/v1",
  );
  const broken = structuredClone(fixture) as {
    views: Array<{ sources: unknown }>;
  };
  if (broken.views[0]) broken.views[0].sources = { "": 3 };
  expect(() => readBundle(broken)).toThrow(
    "atlas bundle: views[overview].sources[]: not Mermaid text",
  );
});

describe("renderTerm", () => {
  const term = {
    id: "term.daemon",
    name: "Daemon",
    definition: "`virgil-desktop`, the always-on process.",
    avoid: [{ word: "agent", except: ["Claude Agent SDK"] }],
    status: "proposed" as const,
    marked: false,
  };

  test("draws the hover card's glossary block with no atlas around it", () => {
    const style = document.createElement("style");
    style.textContent = readFileSync(
      join(import.meta.dir, "../src/styles.css"),
      "utf8",
    );
    document.head.append(style);
    try {
      document.body.innerHTML = '<div id="card"></div>';
      const card = document.getElementById("card") as HTMLElement;
      const block = renderTerm(document, term);
      card.append(block);
      expect(block.className).toBe(GLOSSARY_CLASS);
      expect(block.closest(".virgil-atlas")).toBeNull();
      expect(block.querySelector(".virgil-atlas-term")?.textContent).toBe(
        "Daemon",
      );
      expect(block.querySelector(".virgil-atlas-proposed")?.textContent).toBe(
        "Proposed",
      );
      expect(block.querySelector(".virgil-atlas-def code")?.textContent).toBe(
        "virgil-desktop",
      );
      expect(block.querySelector(".virgil-atlas-avoid")?.textContent).toBe(
        "Avoid agent (except Claude Agent SDK)",
      );
      // The stylesheet reaches it through its own class.
      expect(getComputedStyle(block).display).toBe("grid");
      const chip = block.querySelector(".virgil-atlas-term") as Element;
      expect(getComputedStyle(chip).fontWeight).toBe("600");
    } finally {
      style.remove();
    }
  });
});

// The crate's golden bundle is what the Rust emitter writes; driving the
// clicks from it keeps the viewer's rule and the emitter's sources in step.
describe("the emitter's golden bundle", () => {
  const golden = readBundle(
    JSON.parse(
      readFileSync(
        join(
          import.meta.dir,
          "../../../crates/rosetta-atlas/tests/fixtures/out/bundle.json",
        ),
        "utf8",
      ),
    ),
  );

  test("records areas open from the areas picture and from each other", () => {
    atlas.destroy();
    atlas = mount(host, { bundle: golden });
    atlas.open("records");
    expect(crumbs()).toEqual([golden.project.name, "Records"]);
    click(nodeEl("records-area:conversation"));
    expect(crumbs()).toEqual([
      golden.project.name,
      "Records",
      "Messages and channels",
    ]);
    expect(
      nodeEl("records:message").classList.contains("scope-container"),
    ).toBe(true);
    // An area the conversation's refs cross to is drawn here as its own node.
    click(nodeEl("records-area:people"));
    expect(crumbs()).toEqual([golden.project.name, "Records", "People"]);
    expect(heading()).toBe("People");
    click(nodeEl("records:principal"));
    expect(crumbs()).toEqual([golden.project.name, "Records", "principal"]);
    expect(q(".virgil-atlas-viewmenu-label")?.textContent).toBe(
      "principal Connections",
    );
    expect(nodeEl("records:principal").classList.contains("is-focus")).toBe(
      true,
    );
  });

  test("Connections is never in the menu, and every source it has opens", () => {
    atlas.destroy();
    atlas = mount(host, { bundle: golden });
    expect(host.querySelector('[data-view="connections"]')).toBeNull();
    const connections = golden.views.find((v) => v.kind === "connections");
    expect(connections).toBeDefined();
    for (const id of Object.keys(connections?.sources ?? {})) {
      atlas.open("connections", id);
      expect(nodeEl(id).classList.contains("is-focus")).toBe(true);
    }
  });
});
