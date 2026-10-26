/** Prepare the named local Virgil revision in an isolated, disposable clone. */
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const VIRGIL_REVISION = "aece0f5ee1f1f1367b6805a2d45457868533b0d5";
const source = process.argv[2] && resolve(process.argv[2]);
if (!source || !existsSync(source) || !statSync(source).isDirectory()) {
  throw new Error("Usage: bun examples/virgil-pilot/prepare.ts <local-virgil-repo> [new-checkout-path]");
}
const destination = process.argv[3] ? resolve(process.argv[3]) : mkdtempSync(join(tmpdir(), "rosetta-virgil-pilot-"));
if (process.argv[3] && existsSync(destination)) throw new Error(`Checkout path already exists: ${destination}`);
const binary = process.env.ROSETTA_BIN ? resolve(process.env.ROSETTA_BIN) : resolve(import.meta.dir, "../../target/debug/rosetta");
if (!existsSync(binary)) throw new Error(`Build Rosetta first or set ROSETTA_BIN: ${binary}`);

function run(file: string, args: string[], cwd = destination): string {
  try {
    return execFileSync(file, args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  } catch (error) {
    const command = [file, ...args].join(" ");
    const stderr = (error as { stderr?: Buffer }).stderr?.toString() ?? String(error);
    throw new Error(`${command}: ${stderr.trim()}`);
  }
}

run("git", ["cat-file", "-e", `${VIRGIL_REVISION}^{commit}`], source);
run("git", ["clone", "--shared", "--no-checkout", source, destination], source);
run("git", ["checkout", "-b", "pilot/rosetta-glossary-atlas", VIRGIL_REVISION]);
const start = performance.now();
run(binary, ["init", "."]);
const initSeconds = (performance.now() - start) / 1000;
copyFileSync(join(destination, ".virgil/glossary.json"), join(destination, ".rosetta/glossary.json"));
copyFileSync(join(destination, ".virgil/atlas.json"), join(destination, ".rosetta/atlas.json"));
// The legacy Virgil rulebook scopes product-copy vocabulary to these surfaces.
// Keep that explicit in Rosetta so a full scan does not flag technical prose.
writeFileSync(join(destination, ".rosetta/rosetta.toml"), `[lint]
include = [
  "apps/virgil-app/src/renderer/**/*.tsx",
  "apps/portal/src/**/*.tsx",
  "apps/virgil-web/src/**/*.tsx",
  "packages/ui/src/**/*.tsx",
]
exclude = [
  "**/generated/**",
  "**/gallery/**",
  "**/*.cases.tsx",
  "**/fixtures/**",
]

[refs]
rulebook = "whetstone/rulebook.json"

[[name_sources]]
glob = "packages/virgil-schema/src/generated/schemas.json"
pointer = "/x-virgil-collections"

[records]
collections_key = "x-virgil-collections"
scope_key = "x-virgil-scope"
ref_to_key = "x-virgil-ref-to"

[names]
rust_cli = ["crates/virgil-cli/src/cli.rs"]
typescript_cli = []
typescript_ids = ["packages/virgil-schema/src/**/*.ts"]
`);
run(binary, ["atlas", "sync"]);

const modelPath = join(destination, ".rosetta/atlas.json");
const model = JSON.parse(readFileSync(modelPath, "utf8"));
const viewer = model.nodes.find((node: { id: string }) => node.id === "packages-virgil-atlas-viewer");
if (!viewer || !model.nodes.some((node: { id: string }) => node.id === "crates-virgil-atlas")) {
  throw new Error("The named Virgil revision no longer has the pilot owning parts");
}
viewer.summary = "Renders diagrams and automatically rewrites glossary definitions when a view opens.";
model.capabilities ??= [];
model.capabilities.push({
  id: "glossary-atlas",
  name: "Explain the repository with shared vocabulary",
  purpose: "People and agents use reviewed terms, a repository map and diagrams to find the code responsible for an explanation.",
  status: "proposed",
  terms: ["term.atlas", "term.rulebook"],
  parts: ["crates-virgil-atlas", "packages-virgil-atlas-viewer"],
  implementation: [
    { part: "crates-virgil-atlas", path: "crates/virgil-atlas/src/glossary/render.rs" },
    { part: "crates-virgil-atlas", path: "crates/virgil-atlas/src/atlas/derive.rs" },
    { part: "packages-virgil-atlas-viewer", path: "packages/virgil-atlas-viewer/src/viewer.ts" },
  ],
});
writeFileSync(modelPath, `${JSON.stringify(model, null, 2)}\n`);
copyFileSync(join(import.meta.dir, "assertions.json"), join(destination, ".rosetta/assertions.json"));
run(binary, ["glossary", "render"]);
run(binary, ["atlas", "export"]);
run(binary, ["glossary", "check"]);
run(binary, ["atlas", "check"]);
run("git", ["add", ".rosetta", "ATLAS.md", "GLOSSARY.md"]);
run("git", ["-c", "user.name=Rosetta Pilot", "-c", "user.email=rosetta-pilot@local.invalid", "commit", "-m", "Seed local Rosetta glossary and Atlas journey"]);

const baseline = run("git", ["rev-parse", "HEAD"]);
const report = JSON.parse(run(binary, ["atlas", "changed", "--base", "HEAD", "--format", "json"]));
const proposal = {
  schema: "rosetta.impact-proposal/v1",
  base_commit: baseline,
  base_model_revision: report.base_model_revision,
  intents: [
    { kind: "relationship", from: "packages-virgil-atlas-viewer", to: "packages-virgil-schema", relation: "depends", present: true, rationale: "Make the shared schema an internal workspace dependency available to the viewer." },
    { kind: "relationship", from: "packages-virgil-atlas-viewer", to: "packages-virgil-presence-runtime", relation: "depends", present: true, rationale: "Consider a live presence indicator for collaborators." },
    { kind: "responsibility", part: "packages-virgil-atlas-viewer", value: "Renders Atlas diagrams and glossary terms for inspection without changing source files.", rationale: "Opening a view is read-only." },
    { kind: "claim", text: "Opening a view updates settled glossary definitions automatically.", rationale: "Agent assumption for the person to check." },
  ],
};
const intentPath = join(dirname(destination), `${destination.split("/").at(-1)}.intent.json`);
writeFileSync(intentPath, `${JSON.stringify(proposal, null, 2)}\n`);
console.log(JSON.stringify({ checkout: destination, source_revision: VIRGIL_REVISION, baseline, intent: intentPath, init_seconds: Number(initSeconds.toFixed(2)) }, null, 2));
