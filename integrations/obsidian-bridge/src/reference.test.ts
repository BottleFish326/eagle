import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, symlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { materialMarkdown, parseMaterialUri, vaultEmbedMarkdown } from './reference';

const ID = '0198a7c2-8341-7a31-b842-f15d39f33c18';

test('strictly parses stable material UUID references', () => {
  assert.equal(parseMaterialUri(`material://${ID}`), ID);
  assert.equal(parseMaterialUri('material:///etc/passwd'), undefined);
  assert.equal(parseMaterialUri(`material://${ID}?path=/etc/passwd`), undefined);
  assert.equal(parseMaterialUri('https://example.com/image.png'), undefined);
});

test('generates markdown without machine-specific paths', () => {
  const markdown = materialMarkdown(ID, 'main-logo');
  assert.equal(markdown, `![main-logo](material://${ID})`);
  assert.equal(markdown.includes('/Users/'), false);
});

test('generates a standard vault-relative embed', async (context) => {
  const vault = await mkdtemp(path.join(os.tmpdir(), 'material-vault-'));
  context.after(() => rm(vault, { recursive: true, force: true }));
  const folder = path.join(vault, '素材 images');
  const asset = path.join(folder, 'logo.png');
  await mkdir(folder);
  await writeFile(asset, Buffer.from([0x89, 0x50, 0x4e, 0x47]));

  assert.equal(await vaultEmbedMarkdown(vault, asset), '![[素材 images/logo.png]]');
});

test('rejects a vault embed that resolves outside the vault', async (context) => {
  if (process.platform === 'win32') {
    context.skip('Windows symlink creation requires environment-specific privileges');
    return;
  }
  const vault = await mkdtemp(path.join(os.tmpdir(), 'material-vault-'));
  const outside = await mkdtemp(path.join(os.tmpdir(), 'material-outside-'));
  context.after(() => Promise.all([
    rm(vault, { recursive: true, force: true }),
    rm(outside, { recursive: true, force: true }),
  ]));
  const outsideAsset = path.join(outside, 'secret.png');
  const linkedAsset = path.join(vault, 'linked.png');
  await writeFile(outsideAsset, Buffer.from([0x89, 0x50, 0x4e, 0x47]));
  await symlink(outsideAsset, linkedAsset);

  await assert.rejects(vaultEmbedMarkdown(vault, linkedAsset), /escapes its authorized root/);
});
