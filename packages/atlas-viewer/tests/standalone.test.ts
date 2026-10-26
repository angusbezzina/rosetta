import { afterEach, expect, test } from "bun:test";
import fixture from "../demo/fixture-bundle.json" with { type: "json" };
import { DEFAULT_BRAND } from "../src/brand.ts";
import { boot } from "../src/standalone.ts";
import type { AtlasBundle, FocusedView } from "../src/types.ts";

const realFetch = globalThis.fetch;
const initialUrl = window.location.href;

afterEach(() => {
  globalThis.fetch = realFetch;
  delete window.__ATLAS__;
  window.history.replaceState(null, "", initialUrl);
});

const serve = (routes: Record<string, unknown>) => {
  globalThis.fetch = (async (input: string | URL | Request) => {
    const path = String(input);
    return path in routes
      ? Response.json(routes[path])
      : new Response("", { status: 404 });
  }) as typeof fetch;
};

test("boot reads the bundle and the brand the CLI serves beside the page", async () => {
  serve({
    "./bundle.json": fixture,
    "./brand.json": {
      primary: "#123456",
      secondary: "#234567",
      tertiary: "#345678",
    },
  });
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  expect(el.style.getPropertyValue("--atlas-surface")).toBe("#123456");
  expect(el.style.getPropertyValue("--atlas-store")).toBe("#345678");
  expect(el.querySelector("[data-atlas-id]")).not.toBeNull();
  expect(atlas?.panelState().trail.at(-1)).toBe("daemon");
  const focus = [...el.querySelectorAll<HTMLButtonElement>(".rosetta-detail button")]
    .find((button) => button.textContent?.startsWith("Focus diagram on"));
  expect(focus?.textContent).toContain("connections");
  focus?.click();
  expect(el.textContent).toContain("Daemon Connections");
  atlas?.destroy();
});

test("focusing connections gives the map the full narrow width", async () => {
  serve({ "./bundle.json": fixture });
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  atlas?.setPanelOpen(true);
  const width = window.innerWidth;
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 900 });
  try {
    [...el.querySelectorAll<HTMLButtonElement>(".rosetta-detail button")]
      .find((button) => button.textContent?.startsWith("Focus diagram on"))?.click();
    expect(el.textContent).toContain("Daemon Connections");
    expect(atlas?.panel.hidden).toBe(true);
  } finally {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
    atlas?.setPanelOpen(true);
    atlas?.destroy();
  }
});

test("boot drains a large bundle before a queued brand response", async () => {
  let bodyRead = false;
  let releaseBrand: ((response: Response) => void) | undefined;
  globalThis.fetch = (async (input: string | URL | Request) => {
    const path = String(input);
    if (path === "./bundle.json") {
      return {
        ok: true,
        json: async () => {
          bodyRead = true;
          releaseBrand?.(Response.json({ primary: "#123456" }));
          return fixture;
        },
      } as Response;
    }
    if (path === "./brand.json") {
      return new Promise<Response>((resolve) => {
        releaseBrand = resolve;
        if (bodyRead) resolve(Response.json({ primary: "#123456" }));
      });
    }
    return new Response("", { status: 404 });
  }) as typeof fetch;
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await Promise.race([
    boot(el),
    new Promise<never>((_, reject) => setTimeout(() => reject(new Error("bundle body was not consumed")), 1000)),
  ]);
  expect(bodyRead).toBe(true);
  expect(el.style.getPropertyValue("--atlas-surface")).toBe("#123456");
  atlas?.destroy();
});

test("without a brand.json the brand uses neutral defaults", async () => {
  serve({ "./bundle.json": fixture });
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  expect(el.style.getPropertyValue("--atlas-surface")).toBe(
    DEFAULT_BRAND.primary,
  );
  atlas?.destroy();
});

test("a focused view exposes its shared reference and keeps the selected part on return", async () => {
  const live = structuredClone(fixture) as unknown as AtlasBundle;
  live.capabilities = [{
    id: "request-path",
    name: "Request path",
    purpose: "Answer a request",
    status: "proposed",
    terms: ["term.door"],
    parts: ["door", "daemon"],
    implementation: [{ part: "door", path: "crates/virgil-serve/src/main.rs" }],
  }];
  live.snapshot = { repository: "fixture", head: null, dirty: false, model_revision: "revision", fingerprint: "snapshot", coverage: "git_worktree" };
  const focused: FocusedView = {
    schema: "rosetta.focus/v1",
    id: "request-path-component",
    reference: "rv1.fixture-reference",
    snapshot: live.snapshot,
    query: { capability: "request-path", roots: [], paths: [], level: "component", depth: 2 },
    capabilities: live.capabilities,
    nodes: live.nodes,
    edges: live.edges,
    terms: live.terms,
    target_exists: {},
    implementation_links: [],
    diagram: live.views[0]!.sources[""]!,
    omissions: { nodes_over_budget: 0, edges_over_budget: 0, edges_filtered: 0, beyond_depth: 0, implementation_links_not_drawn: 0 },
  };
  serve({
    "./bundle.json": live,
    "./focus.json?capability=request-path&level=component": focused,
  });
  window.history.replaceState(null, "", "?capability=request-path&level=component&select=door");
  document.body.innerHTML = '<nav id="focus-bar" hidden></nav><main id="atlas"></main>';
  const atlas = await boot(document.getElementById("atlas") as HTMLElement);
  expect(atlas?.panelState().trail.at(-1)).toBe("door");
  expect(window.location.search).toContain("view=rv1.fixture-reference");
  const bar = document.getElementById("focus-bar")!;
  expect(bar.textContent).toContain("proposed");
  expect(bar.querySelector(".rosetta-focus-reference code")?.textContent).toBe(focused.reference);
  atlas?.setPanelOpen(false);
  [...bar.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.textContent === "Details & evidence")?.click();
  expect(atlas?.panel.hidden).toBe(false);
  atlas?.select("daemon");
  expect(window.location.search).toContain("select=daemon");
  expect(bar.querySelector<HTMLAnchorElement>("a")?.href).toContain("select=daemon");
  atlas?.destroy();

  window.history.replaceState(null, "", "?select=door");
  document.body.innerHTML = '<main id="atlas"></main>';
  const reopened = await boot(document.getElementById("atlas") as HTMLElement);
  expect(reopened?.panelState().trail.at(-1)).toBe("door");
  reopened?.destroy();
});

test("returning to the full Atlas opens Connections for a part absent from overview", async () => {
  serve({ "./bundle.json": fixture });
  window.history.replaceState(null, "", "?select=cloud-sql");
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  expect(atlas?.panelState().trail.at(-1)).toBe("cloud-sql");
  expect(el.textContent).toContain("Cloud SQL Connections");
  expect(el.querySelector('[data-atlas-id="cloud-sql"].is-selected')).not.toBeNull();
  atlas?.destroy();
});

test("the local overview offers visible focus and evidence routes with the panel closed", async () => {
  serve({
    "./bundle.json": fixture,
    "./session.json": { local: true, token: "local-token", can_undo: false },
  });
  document.body.innerHTML = '<nav id="focus-bar" hidden></nav><main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  atlas?.setPanelOpen(false);
  const bar = document.getElementById("focus-bar")!;
  const focus = [...bar.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.textContent?.startsWith("Focus on Daemon"));
  expect(focus?.textContent).toContain("connections");
  focus?.click();
  expect(el.textContent).toContain("Daemon Connections");
  [...bar.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.textContent === "Details")?.click();
  expect(atlas?.panel.hidden).toBe(false);
  atlas?.destroy();
});

test("an inlined __ATLAS__ needs no fetch", async () => {
  let requests = 0;
  globalThis.fetch = (async (_input: string | URL | Request) => { requests++; throw new Error("static page must not fetch"); }) as unknown as typeof fetch;
  window.__ATLAS__ = { bundle: fixture, brand: { primary: "#0A0B0C" }, staticSite: true };
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  expect(el.style.getPropertyValue("--atlas-surface")).toBe("#0A0B0C");
  expect(el.style.getPropertyValue("--atlas-service")).toBe(
    DEFAULT_BRAND.secondary,
  );
  expect(requests).toBe(0);
  atlas?.destroy();
});

test("a local correction needs preview and static bundles stay read-only", async () => {
  const live = structuredClone(fixture) as Record<string, unknown>;
  live.snapshot = { repository: "fixture", head: null, dirty: false, model_revision: "revision", fingerprint: "snapshot", coverage: "git_worktree" };
  const submitted: unknown[] = [];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const path = String(input);
    if (path === "./bundle.json") return Response.json(live);
    if (path === "./brand.json") return new Response("", { status: 404 });
    if (path === "./session.json") return Response.json({ local: true, token: "local-token", can_undo: false });
    if (path === "./curation/preview") {
      submitted.push(JSON.parse(String(init?.body)));
      return Response.json({ changes: [{ source: "atlas", id: "door", field: "summary", before: "old", after: "corrected" }], affected_files: [".rosetta/atlas.json", "ATLAS.md"] });
    }
    throw new Error(`unexpected request ${path}`);
  }) as typeof fetch;
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  atlas?.select("door");
  expect(el.textContent).toContain("Daemon calls Door");
  expect(el.textContent).toContain("Door writes Postgres cells");
  const correction = el.querySelector<HTMLDetailsElement>("details.rosetta-curation");
  expect(correction?.open).toBe(false);
  if (correction) correction.open = true;
  const choice = el.querySelector<HTMLSelectElement>('[aria-label="What needs correcting?"]');
  expect(choice).not.toBeNull();
  choice!.value = "1";
  choice!.dispatchEvent(new Event("change"));
  const input = el.querySelector<HTMLTextAreaElement>('[aria-label="Part responsibility"]');
  expect(input).not.toBeNull();
  input!.value = "corrected";
  input!.dispatchEvent(new Event("input"));
  const buttons = [...el.querySelectorAll<HTMLButtonElement>(".rosetta-curation-actions button")];
  expect(buttons[1]?.disabled).toBe(true);
  buttons[0]?.click();
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(submitted).toHaveLength(1);
  expect(buttons[1]?.disabled).toBe(false);
  expect(el.textContent).toContain("Before: old");
  input!.value = "another correction";
  input!.dispatchEvent(new Event("input"));
  expect(buttons[1]?.disabled).toBe(true);
  atlas?.destroy();

  serve({ "./bundle.json": live });
  document.body.innerHTML = '<main id="static-atlas"></main>';
  const staticEl = document.getElementById("static-atlas") as HTMLElement;
  const staticAtlas = await boot(staticEl);
  staticAtlas?.select("door");
  expect(staticEl.querySelector(".rosetta-curation")).toBeNull();
  staticAtlas?.destroy();
});

test("a package that owns a capability offers responsibility correction", async () => {
  const live = structuredClone(fixture) as unknown as AtlasBundle;
  const owner = live.nodes.find((node) => node.id === "door")!;
  owner.kind = "package";
  owner.summary = "Opening this view rewrites glossary definitions.";
  live.capabilities = [{
    id: "request-path", name: "Request path", purpose: "Answer a request", status: "proposed",
    terms: [], parts: ["door"], implementation: [],
  }];
  live.snapshot = { repository: "fixture", head: null, dirty: false, model_revision: "revision", fingerprint: "snapshot", coverage: "git_worktree" };
  serve({
    "./bundle.json": live,
    "./session.json": { local: true, token: "local-token", can_undo: false },
  });
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  atlas?.select("door");
  const choice = el.querySelector<HTMLSelectElement>('[aria-label="What needs correcting?"]')!;
  const responsibility = [...choice.options].find((option) => option.textContent === "Part responsibility");
  expect(responsibility).toBeDefined();
  expect([...choice.options].some((option) => option.textContent?.includes("Remove Door from Request path"))).toBe(true);
  choice.value = responsibility!.value;
  choice.dispatchEvent(new Event("change"));
  expect(el.querySelector<HTMLTextAreaElement>('[aria-label="Part responsibility"]')?.value)
    .toBe("Opening this view rewrites glossary definitions.");
  atlas?.destroy();
});

test("a derived file offers proposal review without a misleading direct edit", async () => {
  const live = structuredClone(fixture) as Record<string, unknown>;
  live.snapshot = { repository: "fixture", head: null, dirty: false, model_revision: "revision", fingerprint: "snapshot", coverage: "git_worktree" };
  serve({
    "./bundle.json": live,
    "./session.json": { local: true, token: "local-token", can_undo: false },
  });
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  atlas?.select("f:apps/virgil-app/src/main.ts");
  const correction = el.querySelector<HTMLDetailsElement>("details.rosetta-curation");
  expect(correction?.textContent).toContain("derived from repository structure");
  expect(correction?.querySelector<HTMLButtonElement>(".rosetta-curation-actions button")?.disabled).toBe(true);
  expect(correction?.querySelector<HTMLInputElement>('input[type="file"]')).not.toBeNull();
  atlas?.destroy();
});

test("a local connection opens its source with the active session", async () => {
  const live = structuredClone(fixture) as Record<string, unknown>;
  live.snapshot = { repository: "fixture", head: null, dirty: false, model_revision: "revision", fingerprint: "snapshot", coverage: "git_worktree" };
  const edges = live.edges as Array<Record<string, unknown>>;
  const edge = edges.find((item) => item.from === "daemon" && item.to === "door")!;
  edge.evidence = { basis: "manifest_dependency", source: "apps/door/package.json", revision: "snapshot" };
  const calls: unknown[] = [];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const path = String(input);
    if (path === "./bundle.json") return Response.json(live);
    if (path === "./brand.json") return new Response("", { status: 404 });
    if (path === "./session.json") return Response.json({ local: true, token: "local-token", can_undo: false });
    if (path === "./evidence/read") {
      calls.push({ body: JSON.parse(String(init?.body)), token: new Headers(init?.headers).get("X-Rosetta-Session") });
      return Response.json({ path: "apps/door/package.json", content: '{"name":"door"}', truncated: false });
    }
    throw new Error(`unexpected request ${path}`);
  }) as typeof fetch;
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  atlas?.select("door");
  const evidence = [...el.querySelectorAll<HTMLDetailsElement>(".rosetta-evidence")]
    .find((item) => item.textContent?.includes("apps/door/package.json"));
  expect(evidence).toBeDefined();
  evidence!.open = true;
  evidence!.dispatchEvent(new Event("toggle"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(calls).toEqual([{ body: { path: "apps/door/package.json", fingerprint: "snapshot" }, token: "local-token" }]);
  expect(evidence?.querySelector("pre")?.textContent).toBe('{"name":"door"}');
  atlas?.destroy();

  serve({ "./bundle.json": live });
  document.body.innerHTML = '<main id="static-atlas"></main>';
  const staticEl = document.getElementById("static-atlas") as HTMLElement;
  const staticAtlas = await boot(staticEl);
  staticAtlas?.select("door");
  expect(staticEl.querySelector(".rosetta-evidence")).toBeNull();
  staticAtlas?.destroy();
});

test("stale source evidence offers a view refresh instead of newer bytes", async () => {
  const live = structuredClone(fixture) as unknown as AtlasBundle;
  live.snapshot = { repository: "fixture", head: null, dirty: false, model_revision: "revision", fingerprint: "old-snapshot", coverage: "git_worktree" };
  const edge = live.edges.find((item) => item.from === "daemon" && item.to === "door")!;
  edge.evidence = { basis: "manifest_dependency", source: "apps/door/package.json", extractor: "test", revision: "old-snapshot" };
  globalThis.fetch = (async (input: string | URL | Request) => {
    const path = String(input);
    if (path === "./bundle.json") return Response.json(live);
    if (path === "./brand.json") return new Response("", { status: 404 });
    if (path === "./session.json") return Response.json({ local: true, token: "local-token", can_undo: false });
    if (path === "./evidence/read") return new Response("source changed since this view was made; refresh the view", { status: 409 });
    throw new Error(`unexpected request ${path}`);
  }) as typeof fetch;
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  atlas?.select("door");
  const evidence = [...el.querySelectorAll<HTMLDetailsElement>(".rosetta-evidence")]
    .find((item) => item.textContent?.includes("apps/door/package.json"))!;
  evidence.open = true;
  evidence.dispatchEvent(new Event("toggle"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(evidence.querySelector("pre")?.textContent).toContain("source changed");
  expect(evidence.querySelector<HTMLAnchorElement>("a")?.textContent).toBe("Refresh view");
  atlas?.destroy();
});

test("the local viewer reviews an agent intent file and outlines touched parts", async () => {
  const live = structuredClone(fixture) as Record<string, unknown>;
  live.snapshot = { repository: "fixture", head: null, dirty: false, model_revision: "revision", fingerprint: "snapshot", coverage: "git_worktree" };
  let reviewed = false;
  globalThis.fetch = (async (input: string | URL | Request) => {
    const path = String(input);
    if (path === "./bundle.json") return Response.json(live);
    if (path === "./brand.json") return new Response("", { status: 404 });
    if (path === "./session.json") return Response.json({ local: true, token: "local-token", can_undo: false });
    if (path === "./change/review") {
      reviewed = true;
      return Response.json({ schema: "rosetta.changed/v1", base_commit: "a".repeat(40), snapshot: { fingerprint: "snapshot" }, changed_paths: ["crates/door/src/main.rs"], impacts: [{ path: "crates/door/src/main.rs", parts: [{ id: "door", name: "Door" }] }], proposed: [{ intent: { kind: "relationship" }, status: "unknown", why: "Runtime behavior cannot be verified" }], unexpected: [{ category: "relationship", id: "web -> door", field: "whole", evidence: "Cargo.toml" }], semantic: [] });
    }
    throw new Error(`unexpected request ${path}`);
  }) as typeof fetch;
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  const atlas = await boot(el);
  atlas?.select("door");
  const file = new File([JSON.stringify({ schema: "rosetta.impact-proposal/v1" })], "intent.json", { type: "application/json" });
  const input = el.querySelector<HTMLInputElement>('.rosetta-curation input[type="file"]');
  expect(input).not.toBeNull();
  Object.defineProperty(input, "files", { configurable: true, value: [file] });
  input!.dispatchEvent(new Event("change"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(reviewed).toBe(true);
  expect(el.textContent).toContain("Runtime behavior cannot be verified");
  expect(el.querySelector('[data-atlas-id="door"]')?.classList.contains("is-change-highlighted")).toBe(true);
  atlas?.destroy();
});

test("a bundle.json the server could not build says the server's reason in the page", async () => {
  globalThis.fetch = (async (input: string | URL | Request) =>
    String(input) === "./bundle.json"
      ? new Response("the checkout's tracked files could not be listed\n", {
          status: 500,
          headers: { "content-type": "text/plain" },
        })
      : new Response("", { status: 404 })) as typeof fetch;
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  expect(await boot(el)).toBeNull();
  const notice = el.querySelector('[role="alert"]');
  expect(notice?.textContent).toBe(
    "The atlas could not be read: the checkout's tracked files could not be listed",
  );
  expect(el.querySelector("[data-atlas-id]")).toBeNull();
});

test("a failure with no body names the status instead", async () => {
  serve({});
  document.body.innerHTML = '<main id="atlas"></main>';
  const el = document.getElementById("atlas") as HTMLElement;
  expect(await boot(el)).toBeNull();
  expect(el.textContent).toContain("bundle.json answered 404");
});
