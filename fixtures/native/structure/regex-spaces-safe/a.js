const plain = /a   b/;
const quantified = /a   +b/;
const repeated = /a  b   c/giu;
const unicode = /é  😀/u;
const call = RegExp("a   b");
const construct = new RegExp('a  b', 'g');
const grouped = RegExp(("a   b"), "u");
const escapedLiteral = /a\/  b/;
