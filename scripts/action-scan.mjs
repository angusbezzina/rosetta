import { spawnSync } from 'node:child_process';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { env, stdout } from 'node:process';

const known = new Set(['glossary-check', 'glossary-lint', 'atlas-check']);
const commands = [...new Set((env.ROSETTA_COMMANDS ?? 'glossary-check,glossary-lint,atlas-check')
  .split(',').map(value => value.trim()).filter(Boolean))];
const strict = env.ROSETTA_STRICT === 'true';
const workspace = env.GITHUB_WORKSPACE ?? process.cwd();
const binary = env.ROSETTA_BIN;

function invoke(args) {
  const result = spawnSync(binary, args, { cwd: workspace, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  if (result.error) throw result.error;
  if (result.status === null || result.status > 2) throw new Error(`${args.join(' ')} exited abnormally`);
  return result;
}

function line(text) {
  return String(text).replace(/[\r\n]+/g, ' ').trim().slice(0, 500);
}

function annotation(file, row, column, severity, message) {
  stdout.write(`${file}:${row}:${column}: ${severity}: ${line(message)}\n`);
}

async function main() {
  if (!binary) throw new Error('ROSETTA_BIN is missing');
  if (!commands.length || commands.some(command => !known.has(command))) {
    throw new Error(`commands must be a comma-separated subset of ${[...known].join(', ')}`);
  }
  if (!['true', 'false'].includes(env.ROSETTA_STRICT ?? 'false')) {
    throw new Error('strict must be true or false');
  }
  let findings = false;
  let sarif = '';
  const severity = strict ? 'error' : 'warning';
  if (commands.includes('glossary-check')) {
    const result = invoke(['glossary', 'check']);
    if (result.status === 2) throw new Error(line(result.stderr || result.stdout));
    if (result.status === 1) {
      findings = true;
      annotation('GLOSSARY.md', 1, 1, severity, result.stderr || result.stdout || 'Glossary check failed');
    }
  }
  if (commands.includes('atlas-check')) {
    const result = invoke(['atlas', 'check', '--json']);
    if (result.status === 2) throw new Error(line(result.stderr || result.stdout));
    const report = JSON.parse(result.stdout);
    for (const finding of report.findings ?? []) {
      annotation('.rosetta/atlas.json', 1, 1, severity, `${finding.path}: ${finding.message}`);
    }
    findings ||= result.status === 1 || (report.findings?.length ?? 0) > 0;
  }
  if (commands.includes('glossary-lint')) {
    let args = ['glossary', 'lint', '--format', 'sarif'];
    if (env.ROSETTA_BASE_SHA) args.push('--base', env.ROSETTA_BASE_SHA);
    let result = invoke(args);
    if (result.status === 2 && env.ROSETTA_BASE_SHA) {
      annotation('.rosetta/glossary.json', 1, 1, 'warning',
        'Pull request base is unavailable locally; scanning all tracked copy. Use checkout fetch-depth: 0 for changed-line scope.');
      args = ['glossary', 'lint', '--format', 'sarif'];
      result = invoke(args);
    }
    if (result.status === 2) throw new Error(line(result.stderr || result.stdout));
    const report = JSON.parse(result.stdout);
    for (const run of report.runs ?? []) {
      for (const finding of run.results ?? []) {
        finding.level = severity;
        const location = finding.locations?.[0]?.physicalLocation;
        if (!location?.artifactLocation?.uri) continue;
        annotation(location.artifactLocation.uri, location.region?.startLine ?? 1,
          location.region?.startColumn ?? 1, severity, finding.message?.text ?? finding.ruleId);
        findings = true;
      }
    }
    const directory = await mkdtemp(join(env.RUNNER_TEMP ?? tmpdir(), 'rosetta-sarif-'));
    sarif = join(directory, 'glossary.sarif');
    await writeFile(sarif, JSON.stringify(report));
  }
  if (env.GITHUB_OUTPUT) {
    await writeFile(env.GITHUB_OUTPUT, `sarif=${sarif}\nfailed=${strict && findings}\n`, { flag: 'a' });
  }
  stdout.write(`Rosetta ${findings ? 'found issues' : 'found no issues'}; strict=${strict}.\n`);
}

main().catch(error => {
  process.stderr.write(`Rosetta action: ${error.message}\n`);
  process.exitCode = 1;
});
