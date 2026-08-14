import assert from 'node:assert/strict';
import test from 'node:test';

import { materialMarkdown, parseMaterialUri } from './reference';

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
