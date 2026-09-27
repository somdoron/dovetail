import assert from 'node:assert/strict';
import { readdir, readFile, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { test } from 'node:test';
import { createExports, syncFiles } from './prepare.mjs';
import { readBook, ROOT, SITE, markdownPath } from '../src/lib/book.mjs';
import { rewriteLinks } from '../src/lib/links.mjs';

const page = { file: 'website/content/book/01-first.md', slug: 'book/first', title: 'First', description: 'First chapter.', source: '# First\n\nHello.\n' };
const next = { ...page, file: 'website/content/book/02-next.md', slug: 'book/next', title: 'Next' };

test('repeated generation preserves root and nested outputs and removes stale files', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'dovetail-site-'));
  const url = pathToFileURL(`${directory}/`);
  try {
    const outputs = createExports([page, next]);
    await syncFiles(url, outputs);
    await writeFile(new URL('obsolete.md', url), 'Old chapter');
    await syncFiles(url, outputs);
    for (const [path, expected] of outputs) assert.equal(await readFile(new URL(`.${path}`, url), 'utf8'), expected);
    assert.ok(!(await readdir(directory)).includes('obsolete.md'));
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test('the site and agent index cover every chapter exactly once in book order', async () => {
  const pages = await readBook();
  const expected = (await readdir(new URL('website/content/book/', ROOT))).filter((file) => /^\d.*\.md$/.test(file)).sort();
  assert.deepEqual(pages.filter((entry) => entry.order).map((entry) => entry.file), expected.map((file) => `website/content/book/${file}`));
  assert.equal(new Set(pages.map(markdownPath)).size, pages.length);
  const outputs = createExports(pages);
  assert.equal(outputs.size, pages.length + 2);
  for (const entry of pages) {
    assert.ok(outputs.get('/llms.txt').includes(`(${SITE}${markdownPath(entry)}): ${entry.description}`));
    assert.ok(outputs.get('/llms-full.txt').includes(outputs.get(markdownPath(entry))));
  }
  assert.ok(!outputs.has('/docs/linter-design.md'));
});

test('link destinations change but code examples, labels, and table escaping survive', () => {
  const code = '```dovetail\nlet link = "[next](02-next.md)"\n```';
  const body = [
    '[next](02-next.md#part)', '[reference][n]', '[n]: 02-next.md "Title"',
    '[local](#part)', '[source](../../../dovetail/src/main.rs)', '![image](../../../art.svg)',
    '[external](https://example.com)', '`[next](02-next.md)`', code,
    '| Link |\n| --- |\n| [a \\| b](02-next.md?x=a\\|b) |',
    '😀 [Unicode](<02-next.md?x=1&y=2> "Keep me")',
    '[![image](../../../art.svg)](02-next.md)',
  ].join('\n\n');
  const output = rewriteLinks(body, page, [page, next], 'markdown');
  assert.ok(output.includes(`(${SITE}/book/next.md#part)`));
  assert.ok(output.includes(`[n]: ${SITE}/book/next.md "Title"`));
  assert.ok(output.includes(`(${SITE}/book/first.md#part)`));
  assert.ok(output.includes('(https://github.com/somdoron/dovetail/blob/main/dovetail/src/main.rs)'));
  assert.ok(output.includes('(https://github.com/somdoron/dovetail/raw/main/art.svg)'));
  assert.ok(output.includes('[external](https://example.com)'));
  assert.ok(output.includes('`[next](02-next.md)`'));
  assert.ok(output.includes(code));
  assert.ok(output.includes(`[a \\| b](${SITE}/book/next.md?x=a%7Cb)`));
  assert.ok(output.includes(`😀 [Unicode](<${SITE}/book/next.md?x=1&amp;y=2> "Keep me")`));
  const html = rewriteLinks('[next](02-next.md#part)', page, [page, next], 'html');
  assert.equal(html, '[next](/book/next/#part)');
});

test('every public book URL in the consumer skill has a generated destination', async () => {
  const outputs = createExports(await readBook());
  for (const directory of ['dovetail/ai', 'dovetail/ai/references', 'dovetail/ai/reviews']) {
    for (const file of await readdir(new URL(`${directory}/`, ROOT))) {
      if (!file.endsWith('.md')) continue;
      const text = await readFile(new URL(`${directory}/${file}`, ROOT), 'utf8');
      for (const match of text.matchAll(/https:\/\/dovetaillang\.org([^\s)#]+)/g)) {
        assert.ok(outputs.has(match[1]), `${directory}/${file}: ${match[0]}`);
      }
    }
  }
});
