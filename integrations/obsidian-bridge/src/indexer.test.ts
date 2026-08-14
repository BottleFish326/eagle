import assert from 'node:assert/strict';
import { mkdtemp, mkdir, realpath, rm, symlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { buildAssetIndex } from './indexer';

const ID = '0198a7c2-8341-7a31-b842-f15d39f33c18';

test('indexes an adjacent sidecar inside an authorized root', async (context) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'material-bridge-'));
  context.after(() => rm(root, { recursive: true, force: true }));
  const asset = path.join(root, 'logo.png');
  await writeFile(asset, Buffer.from([0x89, 0x50, 0x4e, 0x47]));
  await writeFile(`${asset}.asset.yml`, sidecar(ID));

  const result = await buildAssetIndex([root]);
  assert.equal(result.problems.length, 0);
  assert.equal(result.assets.get(ID)?.assetPath, await realpath(asset));
});

test('isolates malformed sidecars', async (context) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'material-bridge-'));
  context.after(() => rm(root, { recursive: true, force: true }));
  const asset = path.join(root, 'broken.png');
  await writeFile(asset, Buffer.from([0x89, 0x50, 0x4e, 0x47]));
  await writeFile(`${asset}.asset.yml`, 'schema: [broken\n');

  const result = await buildAssetIndex([root]);
  assert.equal(result.assets.size, 0);
  assert.equal(result.problems.length, 1);
});

test('rejects a symlink that escapes an authorized root', async (context) => {
  if (process.platform === 'win32') {
    context.skip('Windows symlink creation requires environment-specific privileges');
    return;
  }
  const root = await mkdtemp(path.join(os.tmpdir(), 'material-root-'));
  const outside = await mkdtemp(path.join(os.tmpdir(), 'material-outside-'));
  context.after(() => Promise.all([
    rm(root, { recursive: true, force: true }),
    rm(outside, { recursive: true, force: true }),
  ]));
  const outsideAsset = path.join(outside, 'secret.png');
  const linkedAsset = path.join(root, 'linked.png');
  await writeFile(outsideAsset, Buffer.from([0x89, 0x50, 0x4e, 0x47]));
  await symlink(outsideAsset, linkedAsset);
  await writeFile(`${linkedAsset}.asset.yml`, sidecar(ID));

  const result = await buildAssetIndex([root]);
  assert.equal(result.assets.size, 0);
  assert.match(result.problems[0]?.message ?? '', /escapes its authorized root/);
});

test('skips symlinked directories', async (context) => {
  if (process.platform === 'win32') {
    context.skip('Windows symlink creation requires environment-specific privileges');
    return;
  }
  const root = await mkdtemp(path.join(os.tmpdir(), 'material-root-'));
  const outside = await mkdtemp(path.join(os.tmpdir(), 'material-outside-'));
  context.after(() => Promise.all([
    rm(root, { recursive: true, force: true }),
    rm(outside, { recursive: true, force: true }),
  ]));
  await mkdir(path.join(outside, 'nested'));
  const asset = path.join(outside, 'nested', 'secret.png');
  await writeFile(asset, Buffer.from([0x89, 0x50, 0x4e, 0x47]));
  await writeFile(`${asset}.asset.yml`, sidecar(ID));
  await symlink(outside, path.join(root, 'linked-directory'));

  const result = await buildAssetIndex([root]);
  assert.equal(result.assets.size, 0);
  assert.equal(result.problems.length, 0);
});

function sidecar(id: string): string {
  return `schema: 1\nid: ${id}\ntags:\n  - ui/icon\nupdatedAt: 2026-08-14T12:30:00Z\n`;
}
