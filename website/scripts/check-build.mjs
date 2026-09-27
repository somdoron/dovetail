import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { createExports } from './prepare.mjs';
import { readBook, SITE, htmlPath, markdownPath } from '../src/lib/book.mjs';

const dist = new URL('../dist/', import.meta.url);
const pages = await readBook();
for (const [path, expected] of createExports(pages)) {
  assert.equal(await readFile(new URL(`.${path}`, dist), 'utf8'), expected, path);
}
for (const page of pages) {
  const html = await readFile(new URL(`.${htmlPath(page)}index.html`, dist), 'utf8');
  const markdown = markdownPath(page);
  assert.ok(html.includes(`rel="alternate" type="text/markdown" href="${SITE}${markdown}"`), page.file);
  assert.ok(html.includes(`rel="describedby" href="${SITE}/llms.txt"`), page.file);
  assert.ok(html.includes(`href="${markdown}"`), page.file);
  assert.ok(html.includes('Documentation for agents'), page.file);
}
// Check rendered same-site links and fragments, including navigation and the landing page.
const files = await readdir(dist, { recursive: true });
const htmlFiles = files.filter((file) => file.endsWith('.html'));
const documents = new Map(await Promise.all(htmlFiles.map(async (file) => [file, await readFile(new URL(file, dist), 'utf8')])));
for (const [file, html] of documents) {
  for (const match of html.matchAll(/href="([^"<>]+)"/g)) {
    const href = match[1].replaceAll('&amp;', '&');
    const url = new URL(href, `${SITE}/${file.replace(/index\.html$/, '')}`);
    if (url.origin !== SITE) continue;
    if (file === '404.html' && url.pathname === '/404/') continue;
    const path = decodeURIComponent(url.pathname).replace(/^\//, '');
    const target = path.endsWith('/') || path === '' ? `${path}index.html` : path;
    if (url.pathname.startsWith('/_')) continue;
    assert.ok(files.includes(target), `${file}: missing ${href}`);
    if (url.hash && documents.has(target)) {
      const id = decodeURIComponent(url.hash.slice(1));
      assert.ok(documents.get(target).includes(`id="${id}"`), `${file}: missing anchor ${href}`);
    }
  }
}
assert.ok(files.some((file) => file.startsWith('pagefind/')), 'Missing search index');
console.log(`Verified ${pages.length} Markdown exports, discovery links, and ${htmlFiles.length} HTML pages.`);
