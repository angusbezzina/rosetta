import { spawnSync } from 'node:child_process';
import { cp, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { strict as assert } from 'node:assert';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const binary = process.env.ROSETTA_TEST_BIN ?? join(root, 'target', 'debug', process.platform === 'win32' ? 'rosetta.exe' : 'rosetta');

function command(name, args, cwd, env = process.env) {
  const result = spawnSync(name, args, { cwd, env, encoding: 'utf8' });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${name} ${args.join(' ')}: ${result.stderr}`);
  return result;
}

test('composite scan keeps warnings green and fails only with strict input', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'rosetta-action-test-'));
  const repo = join(directory, 'repo');
  await cp(join(root, 'crates/rosetta-atlas/tests/fixtures/repo'), repo, { recursive: true });
  command('git', ['init', '-q'], repo);
  command('git', ['add', '-A'], repo);
  command('git', ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'base'], repo);
  await writeFile(join(repo, 'docs/shipping.md'), 'Use the workspace here.\n');

  for (const strict of ['false', 'true']) {
    const outputs = join(directory, `outputs-${strict}`);
    const env = {
      ...process.env, ROSETTA_BIN: binary, ROSETTA_STRICT: strict,
      ROSETTA_COMMANDS: 'glossary-lint', ROSETTA_BASE_SHA: 'HEAD',
      GITHUB_WORKSPACE: repo, RUNNER_TEMP: directory, GITHUB_OUTPUT: outputs,
    };
    const result = command('node', [join(root, 'scripts/action-scan.mjs')], root, env);
    assert.match(result.stdout, new RegExp(`docs/shipping\\.md:1:[0-9]+: ${strict === 'true' ? 'error' : 'warning'}: Use Group instead of workspace`));
    const lines = (await readFile(outputs, 'utf8')).trim().split('\n');
    assert.equal(lines[1], `failed=${strict}`);
    const sarif = JSON.parse(await readFile(lines[0].slice('sarif='.length), 'utf8'));
    assert.equal(sarif.runs[0].results[0].level, strict === 'true' ? 'error' : 'warning');
    assert.equal(sarif.runs[0].results[0].locations[0].physicalLocation.artifactLocation.uri, 'docs/shipping.md');
  }
});
