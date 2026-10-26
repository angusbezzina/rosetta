import type { AtlasEdge, EdgeKind } from "./types.ts";

/** Model direction names the actor; a read arrow instead follows data to it. */
export const relationVerbs: Record<EdgeKind, string> = {
  depends: "depends on",
  calls: "calls",
  reads: "reads",
  writes: "writes",
  publishes: "publishes to",
  delivers: "delivers to",
  deploys: "deploys",
  contains: "contains",
  refs: "references",
};

export function flowEnds(edge: AtlasEdge): { from: string; to: string } {
  return edge.kind === "reads"
    ? { from: edge.to, to: edge.from }
    : { from: edge.from, to: edge.to };
}
