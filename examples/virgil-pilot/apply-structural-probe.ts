/** Apply a small manifest-only probe after the participant reviews the intent. */
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const checkout = process.argv[2] && resolve(process.argv[2]);
const intentPath = process.argv[3] && resolve(process.argv[3]);
if (!checkout || !intentPath) throw new Error("Usage: bun examples/virgil-pilot/apply-structural-probe.ts <isolated-checkout> <intent-json>");
const binary = process.env.ROSETTA_BIN ? resolve(process.env.ROSETTA_BIN) : resolve(import.meta.dir, "../../target/debug/rosetta");
if (!existsSync(binary)) throw new Error(`Build Rosetta first or set ROSETTA_BIN: ${binary}`);
function run(file: string, args: string[]): string {
  try {
    return execFileSync(file, args, { cwd: checkout, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  } catch (error) {
    const stderr = (error as { stderr?: Buffer }).stderr?.toString() ?? String(error);
    throw new Error(`${[file, ...args].join(" ")}: ${stderr.trim()}`);
  }
}
if (run("git", ["branch", "--show-current"]) !== "pilot/rosetta-glossary-atlas") {
  throw new Error("The structural probe runs only in the isolated pilot branch");
}
const intent = JSON.parse(readFileSync(intentPath, "utf8"));
const manifestPath = join(checkout, "packages/virgil-atlas-viewer/package.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
if (manifest.name !== "@naos-intelligence/virgil-atlas-viewer" || !existsSync(join(checkout, ".rosetta/assertions.json"))) {
  throw new Error("This is not the prepared Virgil pilot checkout");
}
manifest.dependencies["@naos-intelligence/virgil-schema"] = "workspace:*";
manifest.dependencies["@naos-intelligence/virgil-observability"] = "workspace:*";
manifest.dependencies = Object.fromEntries(Object.entries(manifest.dependencies).sort(([left], [right]) => left.localeCompare(right)));
writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
run(binary, ["atlas", "sync"]);
run(binary, ["atlas", "check"]);
const review = JSON.parse(run(binary, ["atlas", "changed", "--base", intent.base_commit, "--proposal", intentPath, "--format", "json"]));
const verification = JSON.parse(run(binary, ["atlas", "verify", "--json"]));
const statuses = review.proposed.map((item: { status: string }) => item.status);
if (!statuses.includes("implemented") || !statuses.includes("still_proposed") || !statuses.includes("unknown") || review.unexpected.length < 1) {
  throw new Error("The intended proposal/observed distinction was not reproduced");
}
if (!verification.findings.some((finding: { id: string; status: string }) => finding.id === "viewer-boundary" && finding.status === "violation")) {
  throw new Error("The forbidden dependency violation was not reproduced");
}
console.log(JSON.stringify({
  checkout,
  changed_paths: review.changed_paths,
  proposed_statuses: statuses,
  unexpected: review.unexpected.map((item: { id: string }) => item.id),
  assertions: verification.findings.map((finding: { id: string; status: string }) => ({ id: finding.id, status: finding.status })),
}, null, 2));
