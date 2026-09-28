# Dovetail website

Astro and Starlight publish the language book from this repository. The landing
page lives in `src/pages/index.astro`; chapters live in `content/book/`. Formatting,
container-image guidance, and the grammar are also published from their existing
sources. No second copy of the book needs editing.

## Develop and validate

Use Node.js 22.12+ from the repository root:

```sh
npm --prefix website ci
npm --prefix website run dev
npm --prefix website run check
npm --prefix website run build
npm --prefix website run preview
```

Development and preview default to http://localhost:4321. Search works in the built
preview. Book changes regenerate the site during development. Generated content,
Markdown exports, and build output are ignored by Git. Restart dev after changing
the page catalogue or build scripts.

`src/lib/book.mjs` maps source filenames to stable routes and chapter summaries.
Add a summary when adding a chapter. Links between published chapters are rewritten
to website routes for HTML and absolute Markdown URLs for agents. Other repository
links point back to GitHub. Code examples and inline code are preserved. The VS Code
Dovetail grammar provides syntax highlighting; Rhai examples use Rust highlighting.

`npm run check` tests exports, link rewriting, repeated generation, and skill URL
coverage, then runs Astro diagnostics. `npm run build` also checks the actual
exported files, HTML discovery links, internal links/anchors, and the search index.
Run the repository's Python book and AI checks for executable Dovetail examples.

## Agent-readable documentation

The build produces:

- `/llms.txt`: a chapter index with summaries and Markdown links.
- `/llms-full.txt`: the complete published book and supplements.
- `/book/<slug>.md`: individual chapters, plus `/book.md` for the contents.
- `/guides/<slug>.md` and `/reference/grammar.md`: operational supplements and grammar.

Every book page advertises its Markdown alternative and index through HTML `link`
elements and visible footer links. Exports are generated from the same sources as
HTML, including code examples. The site follows the latest published release (including prereleases) via the
`render` branch; it is not a versioned reference for every compiler release. Consumer guidance explains revision matching
and fallback to the repository before this website is published or when unavailable.

The skill keeps workflow and common pitfalls. Add detailed language explanations
and examples to the book, then link the relevant task reference to that chapter.
`tools/ai-coverage.json` records source-to-reference routing without duplicating
chapter headings or requiring every explanation to be restated in the skill.

## Host the static build

The canonical origin is `https://dovetaillang.org` in `src/lib/book.mjs`, matching
the compiler package's homepage. For a static hosting service, build from the
repository root with `npm --prefix website ci && npm --prefix website run build`
and publish `website/dist/`. The build needs the repository's book, guides, and
VS Code grammar, so do not deploy from an isolated copy of `website/`.

Serve directory indexes for chapter routes and `404.html` for missing pages. Serve
`.md` as `text/markdown; charset=utf-8` and `.txt` as `text/plain; charset=utf-8`.
Keep the exported files as direct responses rather than rewriting them to an HTML
fallback. The site needs no server runtime, external fonts, or account credentials.

The `Website` GitHub Actions workflow checks/builds and uploads a static artifact.
It does not deploy. Hosting, the custom domain, and DNS must be configured separately.
After publishing, verify `/`, `/book/`, `/book/type-system.md`, `/llms.txt`,
`/llms-full.txt`, and a search query on a book page.

### Render configuration

Create a **Static Site** connected to this repository. Use these settings:

| Setting | Value |
| --- | --- |
| Branch | `render` |
| Root Directory | Leave empty (repository root) |
| Build Command | `npm --prefix website ci && npm --prefix website run check && npm --prefix website run build` |
| Publish Directory | `website/dist` |
| Auto-Deploy | Enable for changes to the selected branch |
| Environment: `NODE_VERSION` | `24` (matches the website CI major version) |
| Environment: `SKIP_INSTALL_DEPS` | `true` (the build command installs from the website lockfile) |

Keep the Root Directory empty: the build reads `website/content/book/`, `docs/`, `grammar.md`,
and the VS Code grammar, and its checks read `dovetail/ai/`. Setting it to
`website/` would exclude required sibling files. Render documents this behavior
in its [monorepo guide](https://render.com/docs/monorepo-support).

The environment settings explicitly select the
[Node version](https://render.com/docs/node-version) and disable Render's automatic
dependency installation in favor of our `npm ci` command; see
[static-site dependency installation](https://render.com/docs/static-sites#dependency-installation).
This static build needs no start command, Rust toolchain, or runtime service.

#### Release promotion and installation endpoints

Switch the existing site's branch to `render` once after the first release that
contains these changes. This can be done without dashboard work using the
[Render update-service API](https://api-docs.render.com/reference/update-service)
and [deploy API](https://api-docs.render.com/reference/create-deploy):

```sh
# RENDER_API_KEY and RENDER_SERVICE_ID must be set in the environment.
curl --fail-with-body --request PATCH \
  "https://api.render.com/v1/services/$RENDER_SERVICE_ID" \
  --header "Authorization: Bearer $RENDER_API_KEY" \
  --header 'Content-Type: application/json' \
  --data '{"branch":"render","autoDeploy":"yes","buildFilter":{"paths":[],"ignoredPaths":[]}}'
curl --fail-with-body --request POST \
  "https://api.render.com/v1/services/$RENDER_SERVICE_ID/deploys" \
  --header "Authorization: Bearer $RENDER_API_KEY" \
  --header 'Content-Type: application/json' --data '{}'
```

Updating settings does not itself deploy, so the initial deploy call is required.
Later releases need no Render credential in GitHub Actions.
The release workflow advances this branch to the exact release commit only after
binaries, checksums, and the extension have been uploaded, and website
checks pass. It skips an older release if a newer one has been published. This
includes prereleases because Dovetail currently publishes preview versions.

Leave **Included Paths** and **Ignored Paths** empty so every promoted release
triggers a build, even if only the compiler version changed. Main-branch changes
continue to be checked in GitHub Actions without deploying to production.

`/install.sh` and `/install.ps1` are real static files generated from
`website/installers/` with their default version pinned to `dovetail/Cargo.toml`.
They download binaries and checksums from that release on GitHub. This needs no
Render redirects, API key, or per-release dashboard changes. An HTML redirect
would not work for `curl | sh` or `irm | iex`.

For an existing site, changing a checked-in Blueprint alone does not update the
site unless it is managed by that Blueprint. Use the API to change its branch
and clear build filters, or change those settings once in the dashboard.

#### Response headers and routing

Configure these custom headers in the static site's Render dashboard:

| Path | Header | Value |
| --- | --- | --- |
| `/*.md` | `Content-Type` | `text/markdown; charset=utf-8` |
| `/**/*.md` | `Content-Type` | `text/markdown; charset=utf-8` |
| `/llms.txt` | `Content-Type` | `text/plain; charset=utf-8` |
| `/llms-full.txt` | `Content-Type` | `text/plain; charset=utf-8` |

Both Markdown patterns are needed: Render distinguishes root files such as
`/book.md` from nested files such as `/book/type-system.md`. Use Render's
[custom header settings](https://render.com/docs/static-site-headers), rather than
relying on the bundled `_headers` file used by other hosts.

Use normal static-file routing. Do not add a catch-all rewrite to `/index.html`:
chapters have their own generated HTML, and agent endpoints must return their
generated text files. Missing pages should use the generated `404.html`.

#### Custom domain and first-deploy checks

Add `dovetaillang.org` under the site's custom domains and apply the DNS records
shown by Render; see its [custom-domain guide](https://render.com/docs/custom-domains).
The `onrender.com` address can be used for initial smoke tests, but canonical URLs,
the sitemap, and absolute agent links already target `https://dovetaillang.org`.
If the production hostname changes, update `SITE` in `src/lib/book.mjs` and the
public links in `dovetail/ai/` and `tools/ai-coverage.json` together.

After deployment, open a book chapter directly, try search, and inspect the agent
responses (substitute the Render hostname when testing before DNS is configured):

```sh
curl --fail -I https://dovetaillang.org/book/type-system.md
curl --fail -I https://dovetaillang.org/llms.txt
curl --fail https://dovetaillang.org/llms.txt
curl --fail https://dovetaillang.org/llms-full.txt
```

Confirm the response types match the table and the bodies contain Markdown/text.
Promote a release to the `render` branch to trigger subsequent automatic builds; Render
builds directly from the repository and does not consume the GitHub Actions artifact.
