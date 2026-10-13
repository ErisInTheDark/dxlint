declare function probe(value: unknown, expected: string): void;
function kinds(x: unknown) {
  if (typeof x === "string") probe(x, "string"); else probe(x, "unknown");
  if ("number" === typeof x) probe(x, "number");
  if (typeof x !== "boolean") {} else probe(x, "boolean");
  if (typeof x === "bigint") probe(x, "bigint");
  if (typeof x === "symbol") probe(x, "symbol");
  if (typeof x === "undefined") probe(x, "undefined");
  if (typeof x === "function") probe(x, "function");
  if (typeof x === "object") probe(x, "object-null");
}
class Box { value = 1; }
function instance(x: unknown) {
  if (x instanceof Box) probe(x, "object");
}
function array(x: string[] | null) {
  if (Array.isArray(x)) probe(x, "array"); else probe(x, "null");
}
function isString(x: string | null): x is string { return x !== null; }
function predicate(x: string | null) {
  if (isString(x)) probe(x, "string"); else probe(x, "null");
}
function opaque<T>(x: T | null) {
  if (x) probe(x, "unknown"); else probe(x, "unknown");
  if (x !== null) probe(x, "unknown");
}
