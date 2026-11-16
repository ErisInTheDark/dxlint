const assert = require("node:assert/strict");
const test = require("node:test");
const { decode, merge } = require("./tokens");

const TS_TYPES = ["namespace", "class", "enum", "interface", "typeParameter", "type", "parameter", "variable", "property", "enumMember", "function", "method", "string", "keyword", "number", "operator"];
const at = (line, character, length, type, modifiers = 0) => ({ line, character, length, type: TS_TYPES.indexOf(type), modifiers });

test("highlights fill in where the other server has no token, and its tokens win", () => {
  // `\tstate quality = 1;` / `\tstate other = 2;`: TypeScript colours the names.
  const theirs = [0, 7, 7, 8, 1, 1, 7, 5, 8, 1];
  const highlights = [
    { line: 0, character: 1, length: 5, tokenType: "keyword" },
    { line: 0, character: 7, length: 7, tokenType: "variable" },
    { line: 1, character: 1, length: 5, tokenType: "keyword" },
    { line: 1, character: 15, length: 1, tokenType: "comment" },
  ];
  assert.deepEqual(decode(merge(theirs, TS_TYPES, highlights)), [
    at(0, 1, 5, "keyword"), at(0, 7, 7, "property", 1), at(1, 1, 5, "keyword"), at(1, 7, 5, "property", 1),
  ]);
});

test("a range request gets only the highlights inside it", () => {
  const highlights = [0, 1, 2].map((line) => ({ line, character: 0, length: 5, tokenType: "keyword" }));
  const range = { start: { line: 1, character: 0 }, end: { line: 1, character: 10 } };
  assert.deepEqual(decode(merge([], TS_TYPES, highlights, range)), [at(1, 0, 5, "keyword")]);
});
