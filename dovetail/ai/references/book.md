# Read the book on demand

The book owns language syntax, API overviews, configuration fields, and worked
examples. This skill owns the working method, common mistakes, and requested
reviews. Read the local task reference for cautions, then fetch only the book
chapters needed to resolve the task; do not load the entire book by default.

1. In a compiler checkout, prefer its `website/content/book/` files. They match the source being
   tested. A consumer project does not necessarily contain the book.
2. Otherwise use the direct Markdown links in the task reference, or discover
   chapters through [llms.txt](https://dovetaillang.org/llms.txt).
   [llms-full.txt](https://dovetaillang.org/llms-full.txt) is for tasks requiring
   the complete reference, not routine skill startup.
3. The website follows the latest published release (including prereleases), which
   may differ from the installed compiler. For a known
   release tag or commit, read `website/content/book/` at that revision in the
   [source repository](https://github.com/somdoron/dovetail/tree/main/website/content/book).
   Older revisions may store the book at repository-root `book/`; use the layout
   at that revision. Do not invent a tag from a version number or treat an outline
   as implemented.
4. If the site is not yet published or is unavailable, use the source repository
   at the intended revision. If offline, continue with local references, compiler
   `--help`, API queries, and available source/tests. Identify missing documentation
   when it prevents a reliable answer; do not guess unfamiliar syntax or APIs.

Chapter slugs correspond to source filenames without their numeric prefix:
`website/content/book/06-type-system.md` is `/book/type-system.md`. The book index is `/book.md`.
Operational supplements are `/guides/formatting.md` and
`/guides/container-images.md`; the grammar is `/reference/grammar.md`.
Links inside exported Markdown resolve to full URLs, including chapter anchors.

Use compiler queries for exact dependency signatures and source/tests for behavioral
contracts. A book example for a newer library does not establish the installed API.
