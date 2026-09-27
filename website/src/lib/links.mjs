import { parse, postprocess, preprocess } from 'micromark';
import { gfm } from 'micromark-extension-gfm';
import { decodeString } from 'micromark-util-decode-string';
import { SITE, REPOSITORY, htmlPath, markdownPath } from './book.mjs';

// Work on destination tokens, so examples, inline code, tables, and prose remain intact.
export function rewriteLinks(body, page, pages, format) {
  const parser = parse({ extensions: [gfm()] });
  const events = postprocess(parser.document().write(preprocess()(body, 'utf8', true)));
  const edits = [];
  let imageDepth = 0;
  for (const [event, token] of events) {
    if (token.type === 'image') imageDepth += event === 'enter' ? 1 : -1;
    const autolink = ['autolinkProtocol', 'literalAutolinkHttp'].includes(token.type);
    if (event !== 'enter' || (!autolink && !['resourceDestinationString', 'definitionDestinationString'].includes(token.type))) continue;
    const start = token.start.offset;
    const end = token.end.offset;
    const raw = body.slice(start, end);
    const original = autolink ? raw : decodeString(raw);
    const destination = resolveLink(original, page, pages, format, imageDepth > 0);
    if (destination === original) continue;
    const escaped = destination.replace(/[()<>\\|]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`);
    edits.push({ start, end, text: autolink ? escaped : escaped.replaceAll('&', '&amp;') });
  }
  for (const edit of edits.sort((a, b) => b.start - a.start)) {
    body = body.slice(0, edit.start) + edit.text + body.slice(edit.end);
  }
  return body;
}

function resolveLink(link, page, pages, format, isImage) {
  if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(link)) return link;
  const source = new URL(link, `https://source.invalid/${page.file}`);
  const target = pages.find((candidate) => `/${candidate.file}` === source.pathname);
  if (target && !isImage) {
    const path = format === 'markdown' ? SITE + markdownPath(target) : htmlPath(target);
    return path + source.search + source.hash;
  }
  // Internal implementation/design links still work without publishing them as book chapters.
  return `${REPOSITORY}/${isImage ? 'raw' : 'blob'}/main${source.pathname}${source.search}${source.hash}`;
}
