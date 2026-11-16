// dxlint's highlights as semantic tokens: on their own, or merged into
// another server's (an editor shows one provider's tokens per file).

// LSP token data (5 numbers a token, positions relative to the previous
// one) as absolute tokens.
function decode(data) {
  const tokens = [];
  let line = 0, character = 0;
  for (let i = 0; i + 4 < data.length; i += 5) {
    line += data[i];
    character = data[i] === 0 ? character + data[i + 1] : data[i + 1];
    tokens.push({ line, character, length: data[i + 2], type: data[i + 3], modifiers: data[i + 4] });
  }
  return tokens;
}

// Absolute tokens, sorted, as LSP token data.
function encode(tokens) {
  const data = [];
  let line = 0, character = 0;
  for (const token of tokens) {
    data.push(token.line - line, token.line === line ? token.character - character : token.character, token.length, token.type, token.modifiers);
    line = token.line;
    character = token.character;
  }
  return data;
}

const byPosition = (a, b) => a.line - b.line || a.character - b.character;

// `highlights` (`dxlint/highlights` tokens: `{ line, character, length,
// tokenType }`) as absolute tokens of `tokenTypes`; types it lacks are left
// out, as are tokens outside `range` (`{ start, end }` positions) if given.
function fromHighlights(highlights, tokenTypes, range) {
  const inside = (h) => !range
    || (h.line > range.start.line || h.line === range.start.line && h.character + h.length > range.start.character)
    && (h.line < range.end.line || h.line === range.end.line && h.character < range.end.character);
  return highlights
    .filter(inside)
    .map((h) => ({ line: h.line, character: h.character, length: h.length, type: tokenTypes.indexOf(h.tokenType), modifiers: 0 }))
    .filter((token) => token.type >= 0);
}

// `data` (another server's tokens) with `highlights` added where it has no
// token: its tokens (names it resolved) win over dxlint's syntax colours.
function merge(data, tokenTypes, highlights, range) {
  const theirs = decode(data);
  const overlaps = (a, b) => a.line === b.line && a.character < b.character + b.length && b.character < a.character + a.length;
  const ours = fromHighlights(highlights, tokenTypes, range).filter((token) => !theirs.some((other) => overlaps(token, other)));
  return encode([...theirs, ...ours].sort(byPosition));
}

module.exports = { decode, encode, fromHighlights, merge };
