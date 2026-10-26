import { createHash } from 'node:crypto';
import { chmod, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { arch, platform, stdout, env } from 'node:process';
import { execFileSync } from 'node:child_process';

const target = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-arm64': 'aarch64-unknown-linux-musl',
  'linux-x64': 'x86_64-unknown-linux-musl',
  'win32-x64': 'x86_64-pc-windows-msvc',
}[`${platform}-${arch}`];

async function main() {
  let binary;
  if (env.ROSETTA_BINARY_PATH) {
    binary = resolve(env.ROSETTA_BINARY_PATH);
    await readFile(binary);
  } else {
    if (!target) throw new Error(`no Rosetta release binary for ${platform}-${arch}`);
    const version = env.ROSETTA_VERSION ?? 'v0.1.0';
    if (!/^v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
      throw new Error('version must be a release tag such as v0.1.0');
    }
    const tag = version.startsWith('v') ? version : `v${version}`;
    const asset = `rosetta-${target}.tar.gz`;
    const url = `https://github.com/angusbezzina/rosetta/releases/download/${tag}/${asset}`;
    const [archiveResponse, checksumResponse] = await Promise.all([
      fetch(url), fetch(`${url}.sha256`),
    ]);
    if (!archiveResponse.ok || !checksumResponse.ok) {
      throw new Error(`release ${tag} is missing ${asset} or its checksum`);
    }
    const bytes = Buffer.from(await archiveResponse.arrayBuffer());
    const expected = (await checksumResponse.text()).trim().split(/\s+/)[0];
    if (!/^[0-9a-f]{64}$/i.test(expected)) throw new Error('release checksum is malformed');
    const actual = createHash('sha256').update(bytes).digest('hex');
    if (actual.toLowerCase() !== expected.toLowerCase()) throw new Error('release checksum mismatch');
    const directory = await mkdtemp(join(env.RUNNER_TEMP ?? tmpdir(), 'rosetta-action-'));
    const archive = join(directory, asset);
    await writeFile(archive, bytes);
    execFileSync('tar', ['-xzf', archive, '-C', directory], { stdio: 'pipe' });
    binary = join(directory, platform === 'win32' ? 'rosetta.exe' : 'rosetta');
    await readFile(binary);
    if (platform !== 'win32') await chmod(binary, 0o755);
  }
  if (env.GITHUB_OUTPUT) await writeFile(env.GITHUB_OUTPUT, `path=${binary}\n`, { flag: 'a' });
  stdout.write(`Rosetta binary: ${binary}\n`);
}

main().catch(error => {
  process.stderr.write(`Rosetta install: ${error.message}\n`);
  process.exitCode = 1;
});
