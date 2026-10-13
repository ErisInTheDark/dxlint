declare function probe(value: unknown, expected: string): void;
function truth(x: string | null | undefined) {
  if (x) probe(x, "string"); else probe(x, "string-null-undefined");
  if (!x) probe(x, "string-null-undefined"); else probe(x, "string");
  x ? probe(x, "string") : probe(x, "string-null-undefined");
  x && probe(x, "string");
  x || probe(x, "string-null-undefined");
  x ?? probe(x, "null-undefined");
  if (x && typeof x === "string") probe(x, "string");
  if (!x || typeof x !== "string") {} else probe(x, "string");
}
function nullish(x: string | null | undefined) {
  if (x !== null) probe(x, "string-undefined"); else probe(x, "null");
  if (null === x) probe(x, "null"); else probe(x, "string-undefined");
  if (x != null) probe(x, "string"); else probe(x, "null-undefined");
  if (x === undefined) probe(x, "undefined"); else probe(x, "string-null");
  if (undefined != x) probe(x, "string"); else probe(x, "null-undefined");
}
function early(x: string | null) {
  if (!x) return;
  probe(x, "string");
}
function throwing(x: string | null) {
  if (x === null) throw new Error();
  probe(x, "string");
}
function boolean(x: boolean) {
  if (x) probe(x, "true"); else probe(x, "false");
}
