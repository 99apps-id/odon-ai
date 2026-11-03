#!/usr/bin/env node

// Cross-platform local equivalent of stage-sidecar.sh. It loads the ignored
// developer .env files into the cargo build environment so OAuth public-client
// credentials are stamped into odond, then writes the exact externalBin name
// Tauri expects for the host triple.
import { readFileSync, existsSync, mkdirSync, copyFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

for (const name of ['.env', '.env.local']) {
  const path = join(root, name);
  if (!existsSync(path)) continue;
  for (const rawLine of readFileSync(path, 'utf8').replace(/^\uFEFF/, '').split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith('#')) continue;
    const separator = line.indexOf('=');
    if (separator <= 0) continue;
    const key = line.slice(0, separator).trim();
    let value = line.slice(separator + 1).trim();
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    if (!(key in process.env)) process.env[key] = value;
  }
}

const rustc = spawnSync('rustc', ['-vV'], { encoding: 'utf8' });
if (rustc.status !== 0) {
  process.stderr.write(rustc.stderr || 'failed to inspect Rust host triple\n');
  process.exit(rustc.status ?? 1);
}
const triple = rustc.stdout.match(/^host:\s+(.+)$/m)?.[1]?.trim();
if (!triple) throw new Error('rustc did not report a host triple');

const cargo = spawnSync('cargo', ['build', '--release', '--manifest-path', join(root, 'Cargo.toml'), '-p', 'odond'], {
  cwd: root,
  env: process.env,
  stdio: 'inherit',
});
if (cargo.status !== 0) process.exit(cargo.status ?? 1);

const suffix = triple.includes('windows') ? '.exe' : '';
const source = join(root, 'target', 'release', `odond${suffix}`);
const destinationDir = join(root, 'desktop', 'src-tauri', 'binaries');
const destination = join(destinationDir, `odond-${triple}${suffix}`);
mkdirSync(destinationDir, { recursive: true });
copyFileSync(source, destination);
console.log(`staged ${destination}`);
