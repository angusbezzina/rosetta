import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import fixture from "../demo/fixture-bundle.json" with { type: "json" };
import { PELE_VERSION, parse, RENDER_OPTIONS, render } from "../src/pele.ts";
import { readBundle } from "../src/read.ts";

// dist/pele.js inside the npm tarball of the pinned version, whose integrity is
// sha512-AYmHttE70cwNJVXdfPMCANGivFlmc3bGGpMG0GqH5S5vE3RgtEuLlkqbhRfe94Csv/ZEd1TUbv2dMUHpw4p1Bg==
const PELE_JS_SHA512 =
  "sha512-AKVaO+aAksK9xi4AQSPn+hqet1N6qAmNqy8G4QfokMCBnTRopOzDLbXOFsKr/2EcYdkfMXrOWxkUgt3WxRaZNw==";
const sha512 = (path: string) =>
  `sha512-${createHash("sha512").update(readFileSync(path)).digest("base64")}`;

describe("pele", () => {
  test("is pinned to the version the viewer states", () => {
    const manifest = JSON.parse(
      readFileSync(join(import.meta.dir, "../package.json"), "utf8"),
    ) as { dependencies: Record<string, string> };
    expect(manifest.dependencies.pele).toBe(PELE_VERSION);
  });

  test("the installed renderer and the copy dist/ serves are the tarball's file", () => {
    expect(sha512(Bun.resolveSync("pele", import.meta.dir))).toBe(
      PELE_JS_SHA512,
    );
    expect(sha512(join(import.meta.dir, "../dist/pele.js"))).toBe(
      PELE_JS_SHA512,
    );
  });
});

// The crate's goldens are what the Rust emitter writes (cargo test
// regenerates them); the demo fixture is hand-written to the same contract.
const GOLDEN = join(
  import.meta.dir,
  "../../../crates/rosetta-atlas/tests/fixtures/out/bundle.json",
);

for (const [name, bundle] of [
  ["demo fixture", readBundle(fixture)],
  ["crate golden", readBundle(JSON.parse(readFileSync(GOLDEN, "utf8")))],
] as const) {
  describe(`every ${name} source`, () => {
    const ids = new Set(bundle.nodes.map((n) => n.id));
    const sources = bundle.views.flatMap((view) =>
      Object.entries(view.sources).map(
        ([root, text]) => [`${view.id}/${root || "(none)"}`, text] as const,
      ),
    );

    test("there are sources", () => {
      expect(sources.length).toBeGreaterThan(3);
    });

    test.each(sources)("%s parses", (_, text) => {
      expect(parse(text, RENDER_OPTIONS).mermaidType).toBe("flowchart-v2");
    });

    test.each(sources)("%s links every node to an atlas id", (_, text) => {
      const { links } = render(text, RENDER_OPTIONS);
      const clicks = text.match(/^\s+click /gm) ?? [];
      expect(links.length).toBe(clicks.length);
      for (const link of links) {
        expect(
          ids.has(decodeURIComponent(link.href.replace(/^atlas:/, ""))),
        ).toBe(true);
      }
    });
  });
}
