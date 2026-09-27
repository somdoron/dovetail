import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { prepare } from './scripts/prepare.mjs';
import { ROOT, SITE, REPOSITORY } from './src/lib/book.mjs';

await prepare();
const grammar = JSON.parse(await readFile(new URL('../vscode-dovetail/syntaxes/dovetail.tmLanguage.json', import.meta.url), 'utf8'));
grammar.name = 'dovetail';

export default defineConfig({
  site: SITE,
  publicDir: './.generated/public',
  integrations: [
    {
      name: 'dovetail-book',
      hooks: {
        'astro:server:setup': ({ server }) => {
          const sources = ['website/content/book/', 'docs/', 'grammar.md'].map((path) => fileURLToPath(new URL(path, ROOT)));
          server.watcher.add(sources);
          let pending = Promise.resolve();
          server.watcher.on('all', (event, path) => {
            if (['add', 'change', 'unlink'].includes(event) && sources.some((source) => path.startsWith(source))) {
              pending = pending.then(() => prepare()).catch((error) => server.config.logger.error(String(error)));
            }
          });
        },
      },
    },
    starlight({
      title: 'Dovetail',
      description: 'A language for business logic.',
      social: [{ icon: 'github', label: 'GitHub', href: REPOSITORY }],
      customCss: ['./src/styles/docs.css'],
      components: {
        Footer: './src/components/DocsFooter.astro',
        ThemeProvider: './src/components/DarkTheme.astro',
        ThemeSelect: './src/components/ThemeSelect.astro',
      },
      expressiveCode: { shiki: { langs: [grammar], langAlias: { rhai: 'rust' } } },
      sidebar: [
        { label: 'Book overview', link: '/book/' },
        { label: 'The language book', items: [{ autogenerate: { directory: 'book' } }] },
        { label: 'Practical guides', items: [{ autogenerate: { directory: 'guides' } }] },
        { label: 'Reference', items: [{ autogenerate: { directory: 'reference' } }] },
      ],
    }),
  ],
});
