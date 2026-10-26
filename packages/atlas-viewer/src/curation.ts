import type { AtlasBundle, AtlasNode } from "./types.ts";
import type { AtlasHandle } from "./viewer.ts";

/** Present only when the loopback viewer is serving this repository. */
export interface LocalSession {
  local: boolean;
  token: string;
  can_undo: boolean;
  review?: ChangeReport;
}

type Edit = { label: string; kind: string; id?: string; capability?: string; part?: string; initial?: string; mode: "text" | "term" | "toggle" };
type Preview = { changes: Array<{ source: string; id: string; field: string; before: unknown; after: unknown }>; affected_files: string[] };
type ChangeReport = {
  schema: "rosetta.changed/v1";
  base_commit: string;
  snapshot: { fingerprint: string };
  changed_paths: string[];
  impacts: Array<{ path: string; parts: Array<{ id: string; name: string }> }>;
  proposed: Array<{ intent: Record<string, unknown>; status: string; why: string; evidence?: string }>;
  unexpected: Array<{ category: string; id: string; field: string; evidence?: string }>;
  semantic: Array<{ category: string; id: string; field: string; origin: string; evidence?: string }>;
};

const stringify = (value: unknown) => value === null ? "none" : typeof value === "string" ? value : JSON.stringify(value);

async function request<T>(path: string, session: LocalSession, body: unknown): Promise<T> {
  const response = await fetch(`./${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Rosetta-Session": session.token },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error((await response.text()).trim() || `Server answered ${response.status}`);
  return await response.json() as T;
}

function reloadAfterModelEdit(): void {
  const url = new URL(window.location.href);
  if (url.searchParams.has("view")) {
    url.searchParams.set("refresh", "1");
    window.history.replaceState(null, "", url);
  }
  window.location.reload();
}

/** Read a bounded source excerpt through the same protected local session. */
export function evidenceDetails(doc: Document, session: LocalSession, source: string, fingerprint: string): HTMLDetailsElement {
  const details = doc.createElement("details");
  details.className = "rosetta-evidence";
  const summary = doc.createElement("summary");
  summary.textContent = `Open source · ${source}`;
  const code = doc.createElement("pre");
  details.append(summary, code);
  let loaded = false;
  details.addEventListener("toggle", async () => {
    if (!details.open || loaded) return;
    loaded = true;
    code.textContent = "Loading source…";
    try {
      const response = await request<{ path: string; content: string; truncated: boolean }>("evidence/read", session, { path: source, fingerprint });
      code.textContent = response.content + (response.truncated ? "\n… source preview truncated at 128 KiB" : "");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      code.textContent = message;
      if (message.includes("refresh the view")) {
        const refresh = doc.createElement("a");
        refresh.textContent = "Refresh view";
        const url = new URL(window.location.href);
        if (url.searchParams.has("view")) url.searchParams.set("refresh", "1");
        refresh.href = url.href;
        details.append(refresh);
      }
      loaded = false;
    }
  });
  return details;
}

/** Keep mutation controls out of static exports and host-supplied panels. */
export function mountCuration(root: HTMLElement, bundle: AtlasBundle, node: AtlasNode | undefined, session: LocalSession, doc: Document, atlas: AtlasHandle): void {
  const element = <K extends keyof HTMLElementTagNameMap>(tag: K, text?: string): HTMLElementTagNameMap[K] => {
    const item = doc.createElement(tag);
    if (text !== undefined) item.textContent = text;
    return item;
  };
  const section = element("details");
  section.className = "rosetta-curation";
  section.append(element("summary", session.can_undo ? "Correct the shared model · undo available" : "Correct the shared model"));
  if (session.can_undo) {
    const undo = element("button", "Undo my last applied correction");
    undo.type = "button";
    const status = element("p");
    status.setAttribute("role", "status");
    undo.addEventListener("click", async () => {
      undo.disabled = true;
      try {
        await request("curation/undo", session, {});
        reloadAfterModelEdit();
      } catch (error) {
        status.textContent = error instanceof Error ? error.message : String(error);
        undo.disabled = false;
      }
    });
    section.append(undo, status);
  }
  if (!node || !bundle.snapshot) {
    root.append(section);
    return;
  }

  const term = bundle.terms.find((entry) => entry.id === node.refs.term);
  const isPart = ["surface", "service", "store", "agent", "external"].includes(node.kind);
  // A repository owner can be a runtime part or a concrete code unit; walked
  // files and directories are implementation, not capability owners.
  const isEditableOwner = isPart || ["crate", "package", "app"].includes(node.kind);
  const edits: Edit[] = isEditableOwner ? [
    { label: "Part name", kind: "set_node_label", id: node.id, initial: node.label, mode: "text" },
    { label: "Part responsibility", kind: "set_node_summary", id: node.id, initial: node.summary ?? "", mode: "text" },
    { label: "Linked term", kind: "set_node_term", id: node.id, initial: node.refs.term ?? "", mode: "term" },
  ] : [];
  if (term && isEditableOwner) edits.push(
    { label: `Name of ${term.name}`, kind: "set_term_name", id: term.id, initial: term.name, mode: "text" },
    { label: `Definition of ${term.name}`, kind: "set_term_definition", id: term.id, initial: term.definition, mode: "text" },
  );
  for (const capability of bundle.capabilities ?? []) {
    if (capability.parts.includes(node.id)) edits.push(
      { label: `${capability.name}: capability name`, kind: "set_capability_name", id: capability.id, initial: capability.name, mode: "text" },
      { label: `${capability.name}: purpose`, kind: "set_capability_purpose", id: capability.id, initial: capability.purpose, mode: "text" },
    );
    if (isEditableOwner) edits.push({
      label: `${capability.parts.includes(node.id) ? "Remove" : "Link"} ${node.label} ${capability.parts.includes(node.id) ? "from" : "to"} ${capability.name}`,
      kind: "set_capability_part", capability: capability.id, part: node.id,
      initial: String(!capability.parts.includes(node.id)), mode: "toggle",
    });
  }
  const choice = element("select");
  choice.setAttribute("aria-label", "What needs correcting?");
  for (const [index, edit] of edits.entries()) {
    const option = element("option", edit.label);
    option.value = String(index);
    choice.append(option);
  }
  const valueHost = element("div");
  const rationale = element("textarea");
  rationale.rows = 2;
  rationale.placeholder = "Why is this correction needed?";
  rationale.setAttribute("aria-label", "Reason for correction");
  const importLabel = element("label", "Review an agent proposal (JSON file)");
  const importFile = element("input");
  importFile.type = "file";
  importFile.accept = ".json,application/json";
  importLabel.append(importFile);
  const controls = element("div");
  controls.className = "rosetta-curation-actions";
  const previewButton = element("button", "Preview change");
  const applyButton = element("button", "Apply reviewed change");
  previewButton.type = "button";
  previewButton.disabled = edits.length === 0;
  applyButton.type = "button";
  applyButton.disabled = true;
  const result = element("div");
  result.className = "rosetta-curation-preview";
  result.setAttribute("role", "status");
  controls.append(previewButton, applyButton);
  section.append(choice, valueHost, rationale, importLabel, controls, result);
  root.append(section);
  if (edits.length === 0) {
    choice.hidden = true;
    rationale.hidden = true;
    controls.hidden = true;
  }

  let value: HTMLTextAreaElement | HTMLSelectElement | null = null;
  let reviewed: object | null = null;
  const reset = () => { reviewed = null; applyButton.disabled = true; result.replaceChildren(); };
  const showReview = (report: ChangeReport) => {
    const touched = new Set(report.impacts.flatMap((impact) => impact.parts.map((part) => part.id)));
    atlas.highlight(touched);
    const review = element("div");
    review.append(element("h4", "Proposed versus observed"));
    review.append(element("small", `${report.changed_paths.length} changed paths · ${report.proposed.length} intentions · ${report.unexpected.length} additional structural changes. Dashed outlines mark touched parts in this diagram.`));
    for (const item of report.proposed) {
      const row = element("p");
      row.append(element("strong", `${item.status.replaceAll("_", " ")}: `), doc.createTextNode(`${stringify(item.intent)} — ${item.why}${item.evidence ? ` · ${item.evidence}` : ""}`));
      review.append(row);
      if (item.evidence) review.append(evidenceDetails(doc, session, item.evidence, report.snapshot.fingerprint));
    }
    const unexpected = element("details");
    unexpected.append(element("summary", `Additional observed structure (${report.unexpected.length})`));
    for (const item of report.unexpected) {
      unexpected.append(element("p", `${item.category} ${item.id} ${item.field}${item.evidence ? ` · ${item.evidence}` : ""}`));
      if (item.evidence) unexpected.append(evidenceDetails(doc, session, item.evidence, report.snapshot.fingerprint));
    }
    review.append(unexpected);
    const paths = element("details");
    paths.append(element("summary", `Changed paths (${report.changed_paths.length})`));
    for (const path of report.changed_paths) paths.append(element("p", path));
    review.append(paths);
    review.append(element("small", `Baseline ${report.base_commit.slice(0, 12)} · current ${report.snapshot.fingerprint.slice(0, 12)}. Curation changes remain separate from observed implementation.`));
    result.replaceChildren(review);
  };
  const selectedEdit = () => edits[Number(choice.value)] ?? edits[0]!;
  const drawValue = () => {
    reset();
    if (edits.length === 0) {
      valueHost.replaceChildren(element("p", "This node is derived from repository structure. Select a responsible part to correct the shared model, or review an agent proposal here."));
      return;
    }
    const edit = selectedEdit();
    if (edit.mode === "toggle") {
      value = null;
      valueHost.replaceChildren(element("p", edit.label));
    } else if (edit.mode === "term") {
      const select = element("select");
      select.setAttribute("aria-label", "Linked glossary term");
      const none = element("option", "No linked term");
      none.value = "";
      select.append(none);
      for (const term of bundle.terms) {
        const option = element("option", `${term.name} (${term.id})${term.status === "proposed" ? " · proposed" : ""}`);
        option.value = term.id;
        select.append(option);
      }
      select.value = edit.initial ?? "";
      value = select;
      valueHost.replaceChildren(select);
      select.addEventListener("change", reset);
    } else {
      const textarea = element("textarea");
      textarea.rows = edit.kind === "set_term_definition" || edit.kind === "set_node_summary" ? 3 : 2;
      textarea.value = edit.initial ?? "";
      textarea.setAttribute("aria-label", edit.label);
      value = textarea;
      valueHost.replaceChildren(textarea);
      textarea.addEventListener("input", reset);
    }
  };
  choice.addEventListener("change", drawValue);
  rationale.addEventListener("input", reset);
  drawValue();
  if (session.review) showReview(session.review);
  const proposal = () => {
    const edit = selectedEdit();
    const change = edit.mode === "toggle"
      ? { kind: edit.kind, capability: edit.capability, part: edit.part, present: edit.initial === "true" }
      : edit.mode === "term"
        ? { kind: edit.kind, id: edit.id, term: value?.value || null }
        : { kind: edit.kind, id: edit.id, value: value?.value.trim() ?? "" };
    return { schema: "rosetta.curation/v1", base_model_revision: bundle.snapshot!.model_revision, rationale: rationale.value.trim(), changes: [change] };
  };
  const showPreview = async (submitted: object) => {
    reset();
    previewButton.disabled = true;
    try {
      const preview = await request<Preview>("curation/preview", session, submitted);
      const details = element("div");
      for (const change of preview.changes) {
        details.append(element("p", `${change.source} · ${change.id} · ${change.field}`));
        details.append(element("p", `Before: ${stringify(change.before)}`));
        details.append(element("p", `After: ${stringify(change.after)}`));
      }
      details.append(element("small", `Files: ${preview.affected_files.join(", ")}`));
      result.replaceChildren(details);
      reviewed = submitted;
      applyButton.disabled = false;
    } catch (error) {
      result.textContent = error instanceof Error ? error.message : String(error);
    } finally {
      previewButton.disabled = false;
    }
  };
  previewButton.addEventListener("click", () => { if (edits.length > 0) void showPreview(proposal()); });
  importFile.addEventListener("change", async () => {
    const file = importFile.files?.[0];
    if (!file) return;
    if (file.size > 64 * 1024) { result.textContent = "Proposal exceeds 64 KiB"; return; }
    try {
      const submitted = JSON.parse(await file.text()) as { schema?: string };
      if (submitted.schema === "rosetta.impact-proposal/v1") {
        reset();
        const report = await request<ChangeReport>("change/review", session, submitted);
        if (report.schema !== "rosetta.changed/v1") throw new Error("server did not return a change review");
        session.review = report;
        showReview(report);
      } else {
        session.review = undefined;
        atlas.highlight(new Set());
        await showPreview(submitted);
      }
    } catch (error) {
      result.textContent = error instanceof Error ? error.message : String(error);
    }
  });
  applyButton.addEventListener("click", async () => {
    if (!reviewed) return;
    applyButton.disabled = true;
    try {
      await request("curation/apply", session, reviewed);
      reloadAfterModelEdit();
    } catch (error) {
      result.textContent = error instanceof Error ? error.message : String(error);
      reviewed = null;
    }
  });
}
