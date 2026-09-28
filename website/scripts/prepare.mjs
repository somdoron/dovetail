import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { SITE, REPOSITORY, readBook, htmlPath, markdownPath } from '../src/lib/book.mjs';
import { rewriteLinks } from '../src/lib/links.mjs';

export function createExports(pages) {
  const outputs = new Map();
  for (const page of pages) {
    const body = rewriteLinks(page.source, page, pages, 'markdown');
    const firstBreak = body.indexOf('\n');
    outputs.set(markdownPath(page), `${body.slice(0, firstBreak)}\n\nSource: ${SITE}${htmlPath(page)}\n${body.slice(firstBreak)}\n`);
  }
  outputs.set('/llms.txt', [
    '# Dovetail', '',
    '> A language for business logic, compiling to WebAssembly with typed errors and structured concurrency.', '',
    'Read only the relevant Markdown chapters below. This site follows main; use the',
    'book at your compiler/library revision when behavior differs. Chapters marked as',
    'design outlines do not imply implemented runtime features.', '',
    `The [complete book](${SITE}/llms-full.txt) is available when the whole reference is needed.`, '',
    '## Book and guides', '',
    ...pages.map((page) => `- [${page.title.replace(/[\\[\]]/g, '\\$&')}](${SITE}${markdownPath(page)}): ${page.description}`), '',
  ].join('\n'));
  outputs.set('/llms-full.txt', [...outputs.entries()].filter(([path]) => path.endsWith('.md')).map(([, value]) => value).join('\n---\n\n'));
  return outputs;
}

export async function prepare() {
  const pages = await readBook();
  const docs = new Map();
  for (const page of pages) {
    const metadata = {
      title: page.title.replace(/^Part \d+: /, ''),
      description: page.description,
      editUrl: `${REPOSITORY}/edit/main/${page.file}`,
      sidebar: { order: page.order },
      head: [
        { tag: 'link', attrs: { rel: 'alternate', type: 'text/markdown', href: SITE + markdownPath(page) } },
        { tag: 'link', attrs: { rel: 'describedby', href: SITE + '/llms.txt' } },
      ],
    };
    const body = rewriteLinks(page.source.replace(/^# .+\n/, ''), page, pages, 'html');
    docs.set(`/${page.slug}.md`, `---\n${JSON.stringify(metadata, null, 2)}\n---\n${body}`);
  }
  await syncFiles(new URL('../src/content/docs/', import.meta.url), docs);
  const publicFiles = createExports(pages);
  for (const file of await readdir(new URL('../static/', import.meta.url))) {
    publicFiles.set(`/${file}`, await readFile(new URL(`../static/${file}`, import.meta.url)));
  }
  await syncFiles(new URL('../.generated/public/', import.meta.url), publicFiles);
  return pages;
}

export async function syncFiles(directory, files) {
  await mkdir(directory, { recursive: true });
  const existing = await readdir(directory, { recursive: true, withFileTypes: true });
  const expected = new Set();
  for (const [path, body] of files) {
    const destination = fileURLToPath(new URL(`.${path}`, directory));
    expected.add(destination);
    await mkdir(dirname(destination), { recursive: true });
    const content = Buffer.isBuffer(body) ? body : Buffer.from(body);
    const previous = await readFile(destination).catch((error) => {
      if (error.code !== 'ENOENT') throw error;
    });
    if (!previous?.equals(content)) await writeFile(destination, content);
  }
  for (const entry of existing) {
    const path = join(entry.parentPath, entry.name);
    if (entry.isFile() && !expected.has(path)) await rm(path);
  }
}
