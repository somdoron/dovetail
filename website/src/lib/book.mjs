import { readFile, readdir } from 'node:fs/promises';

export const SITE = 'https://dovetaillang.org';
export const REPOSITORY = 'https://github.com/somdoron/dovetail';
export const ROOT = new URL('../../../', import.meta.url);

const summaries = [
  'Install Dovetail, set up your editor, and run your first program.',
  'Compiler commands, workspace manifests, dependencies, and runtime permissions.',
  'Layout, bindings, primitive types, operators, and string interpolation.',
  'Conditional expressions, exhaustive pattern matching, and loops.',
  'Functions, named arguments, closures, and function references.',
  'Records, enums, newtypes, collections, modules, extensions, and interfaces.',
  'Generic functions and types with explicit constraints.',
  'Traits, implementations, inheritance, defaults, and method resolution.',
  'Classes, encapsulation, inheritance, mutable state, and identity.',
  'Option, Result, early returns, and the boundary between failures and defects.',
  'Packages, imports, visibility, and Git dependencies.',
  'Deferred async work, execution, structured concurrency, and cancellation.',
  'Resource acquisition, ownership scopes, composition, and reliable cleanup.',
  'Stream sources, transformations, concurrent merging, and byte transports.',
  'Test declarations, organization, attributes, filtering, and execution.',
  'Built-in derives and custom derive macros written in Rhai.',
  'WebAssembly component dependencies, generated bindings, and SQLite.',
  'Project boundaries, dependency direction, ports, adapters, and composition.',
  'Model business rules with value objects, entities, aggregates, and transitions.',
  'Use cases, repositories, transactions, and obligations; includes design outlines.',
  'Bounded contexts, public contracts, and integration events; includes design outlines.',
  'Available standard-library projects, API areas, and current limitations.',
  'Naming, functional patterns, error handling, and project organization.',
  'Typed string builders, interpolation, parameterized SQL, and custom prefixes.',
  'Tuple extension, recursive implementations, and parser composition.',
  'Advanced bounds, variance, associated types, and generic associated types.',
  'Reproducible images, CI, runtime grants, deployment, and rollback.',
];

export function htmlPath(page) {
  return `/${page.slug}/`;
}

export function markdownPath(page) {
  return `/${page.slug}.md`;
}

export async function readBook() {
  const files = (await readdir(new URL('website/content/book/', ROOT))).filter((file) => /^\d.*\.md$/.test(file)).sort();
  const pages = await Promise.all(files.map((file, index) => readPage(
    `website/content/book/${file}`, `book/${file.replace(/^\d+-/, '').replace(/\.md$/, '')}`,
    summaries[index], index + 1,
  )));
  if (pages.length !== summaries.length) throw new Error('Add a website summary for every book chapter.');
  const extras = [
    ['website/content/book/toc.md', 'book', 'The complete Dovetail language book, organized by chapter.'],
    ['website/content/book/validation.md', 'guides/book-validation', 'How the book’s links and complete examples are validated.'],
    ['grammar.md', 'reference/grammar', 'The lexical and syntactic grammar of Dovetail.'],
    ['docs/formatting.md', 'guides/formatting', 'Canonical formatting and editor integration.'],
    ['docs/container-images.md', 'guides/container-images', 'OCI image configuration, runtime matching, and reproducible builds.'],
  ];
  for (const [file, slug, description] of extras) pages.push(await readPage(file, slug, description));
  return pages;
}

async function readPage(file, slug, description, order = 0) {
  const source = await readFile(new URL(file, ROOT), 'utf8');
  const title = source.match(/^# (.+)$/m)?.[1];
  if (!title || !description) throw new Error(`${file}: missing title or description`);
  return { file, slug, title, description, order, source };
}
