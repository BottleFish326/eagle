import { copyFile, mkdir, readFile, realpath, rm, stat, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';

const MARKER = '.eagle-obsidian-smoke.json';
const ID = '0198a7c2-8341-7a31-b842-f15d39f33c18';
const PNG = Buffer.from([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d,
  0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
  0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00,
  0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00,
  0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
  0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
]);

const [command, ...argumentsList] = process.argv.slice(2);
const options = parseOptions(argumentsList);

if (command === 'setup') {
  await setup(requiredPath(options, 'vault'), requiredPath(options, 'materials'));
} else if (command === 'clean') {
  await clean(requiredPath(options, 'vault'), requiredPath(options, 'materials'));
} else {
  throw new Error('usage: node tools/obsidian-smoke.mjs setup|clean --vault PATH --materials PATH');
}

async function setup(vaultPath, materialsPath) {
  await ensureAbsent(vaultPath);
  await ensureAbsent(materialsPath);

  const pluginSource = path.resolve('integrations/obsidian-bridge');
  const pluginTarget = path.join(vaultPath, '.obsidian/plugins/material-bridge');
  await mkdir(pluginTarget, { recursive: true });
  await mkdir(materialsPath, { recursive: true });

  await Promise.all([
    copyFile(path.join(pluginSource, 'main.js'), path.join(pluginTarget, 'main.js')),
    copyFile(path.join(pluginSource, 'manifest.json'), path.join(pluginTarget, 'manifest.json')),
  ]);

  const externalAsset = path.join(materialsPath, 'external.png');
  await writeFile(externalAsset, PNG);
  await writeFile(
    `${externalAsset}.asset.yml`,
    `schema: 1\nid: ${ID}\ntags:\n  - smoke/obsidian\nupdatedAt: 2026-08-14T00:00:00Z\n`,
  );
  await writeFile(path.join(vaultPath, 'internal.png'), PNG);
  await writeFile(
    path.join(vaultPath, 'smoke.md'),
    `# Material Bridge smoke test\n\nExternal:\n\n![external](material://${ID})\n\nInternal:\n\n![[internal.png]]\n`,
  );
  await writeFile(
    path.join(pluginTarget, 'data.json'),
    `${JSON.stringify({ roots: [await realpath(materialsPath)] }, null, 2)}\n`,
  );
  await writeFile(
    path.join(vaultPath, '.obsidian/community-plugins.json'),
    `${JSON.stringify(['material-bridge'], null, 2)}\n`,
  );

  const marker = `${JSON.stringify({ schema: 1, generator: 'eagle-obsidian-smoke' }, null, 2)}\n`;
  await Promise.all([
    writeFile(path.join(vaultPath, MARKER), marker),
    writeFile(path.join(materialsPath, MARKER), marker),
  ]);
  console.log(`vault=${await realpath(vaultPath)}`);
  console.log(`materials=${await realpath(materialsPath)}`);
  console.log(`id=${ID}`);
}

async function clean(vaultPath, materialsPath) {
  await removeMarkedDirectory(vaultPath);
  await removeMarkedDirectory(materialsPath);
}

async function removeMarkedDirectory(directoryPath) {
  const canonical = await realpath(directoryPath);
  const markerPath = path.join(canonical, MARKER);
  const marker = JSON.parse(await readFile(markerPath, 'utf8'));
  if (marker.generator !== 'eagle-obsidian-smoke') {
    throw new Error(`refusing cleanup: invalid marker in ${canonical}`);
  }
  const forbidden = new Set([path.parse(canonical).root, os.homedir(), process.cwd()]);
  if (forbidden.has(canonical)) {
    throw new Error(`refusing cleanup of broad path: ${canonical}`);
  }
  await rm(canonical, { recursive: true });
  console.log(`removed=${canonical}`);
}

async function ensureAbsent(targetPath) {
  try {
    await stat(targetPath);
    throw new Error(`target must not already exist: ${targetPath}`);
  } catch (error) {
    if (error?.code !== 'ENOENT') {
      throw error;
    }
  }
}

function parseOptions(values) {
  const result = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (key === undefined || value === undefined || !key.startsWith('--')) {
      throw new Error('options must be --name value pairs');
    }
    result.set(key.slice(2), value);
  }
  return result;
}

function requiredPath(optionsMap, name) {
  const value = optionsMap.get(name);
  if (value === undefined || !path.isAbsolute(value)) {
    throw new Error(`--${name} must be an absolute path`);
  }
  return path.normalize(value);
}
