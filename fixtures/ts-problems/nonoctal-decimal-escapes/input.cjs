const eight = "\8";
const nine = '\9';
const escaped = "\\8";
const odd = "\\\8";
const many = "\8\9\8";
const nul = "\0\8";
const octal = "\08";
const unicode = "é\8";
const continued = "\
\8";
const regex = /\8/;
const template = String.raw`\8`;
const safe = "8"; // \9
const nulnine = "\0\9";
const encoded = "\u0038";
const pair = { "\8": "\9" };
const nested = `${"\8"}`;
