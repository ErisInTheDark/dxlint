const escaped = RegExp("\\w  a");
const escapeAfter = new RegExp("a  \\w", "g");
const escapedSpace = RegExp("a \\x20b");
const classes = /[a   b] c d/;
const one = /a b/;
const quantified = /a  +b/;
const unknownFlags = RegExp("a  b", flags);
const template = RegExp(`a  b`);
