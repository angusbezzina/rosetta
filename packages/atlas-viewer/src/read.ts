import {
  ATLAS_BUNDLE_SCHEMA,
  LEGACY_ATLAS_BUNDLE_SCHEMA,
  type AtlasBundle,
} from "./types.ts";

type Json = Record<string, unknown>;

const isObject = (value: unknown): value is Json =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * Accept parsed JSON as an atlas bundle, or throw saying where it is not one.
 * The shape is the Rust crate's to guarantee; this checks what the viewer
 * would otherwise trip over (the schema, the arrays, each view's sources) so
 * a stale or foreign file fails here with a reason rather than mid-render.
 */
export function readBundle(value: unknown): AtlasBundle {
  const fail = (path: string, why: string): never => {
    throw new Error(`atlas bundle: ${path}: ${why}`);
  };
  if (!isObject(value)) return fail("$", "not an object");
  if (value.schema !== ATLAS_BUNDLE_SCHEMA && value.schema !== LEGACY_ATLAS_BUNDLE_SCHEMA) {
    fail("schema", `expected ${ATLAS_BUNDLE_SCHEMA} or ${LEGACY_ATLAS_BUNDLE_SCHEMA}`);
  }
  if (!isObject(value.project) || typeof value.project.name !== "string") {
    fail("project", "needs a name");
  }
  for (const key of ["views", "nodes", "edges", "terms", "rules"] as const) {
    if (!Array.isArray(value[key])) fail(key, "not an array");
  }
  for (const [i, view] of (value.views as unknown[]).entries()) {
    if (!isObject(view) || typeof view.id !== "string") {
      return fail(`views[${i}]`, "needs an id");
    }
    if (!isObject(view.sources)) {
      return fail(`views[${view.id}].sources`, "not an object");
    }
    for (const [root, text] of Object.entries(view.sources)) {
      if (typeof text !== "string") {
        fail(`views[${view.id}].sources[${root}]`, "not Mermaid text");
      }
    }
  }
  for (const [i, node] of (value.nodes as unknown[]).entries()) {
    if (!isObject(node) || typeof node.id !== "string") {
      return fail(`nodes[${i}]`, "needs an id");
    }
    if (!isObject(node.refs)) fail(`nodes[${node.id}].refs`, "not an object");
  }
  return value as unknown as AtlasBundle;
}
