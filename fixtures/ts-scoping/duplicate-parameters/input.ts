function plain(a, a) {}
function many(a, b, a, b, a) {}
function renamed({ source: a }, a) {}
function nested([a, [b, a]]) {}
function shorthand({ a = 0 }, a) {}
function rest(a, ...a) {}
function defaults(a = () => 1, a) {}
function bodyOnly(a) { var a; }
function outer(a, cb = function inner(a, a) {}) {}
const arrow = (a, a) => 0;
class C { method(a, a) {} }
function escaped(\u0061, a) {}
function /* ( */ comments(a, a) {}
function generic<T extends () => void>(a, a) {}
function typed(a, a): () => void { return () => {}; }
declare function declared(a, a): void;
