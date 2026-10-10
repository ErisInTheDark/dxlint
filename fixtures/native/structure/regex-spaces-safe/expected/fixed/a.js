const plain = /a {3}b/;
const quantified = /a {2} +b/;
const repeated = /a {2}b {3}c/giu;
const unicode = /é {2}😀/u;
const call = RegExp("a {3}b");
const construct = new RegExp('a {2}b', 'g');
const grouped = RegExp(("a {3}b"), "u");
const escapedLiteral = /a\/ {2}b/;
