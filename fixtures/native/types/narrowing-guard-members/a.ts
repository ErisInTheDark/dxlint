declare function probe(value: unknown, expected: string): void;
function discriminant(x: { kind: "text"; value: string } | { kind: "count"; value: number }) {
  if (x.kind === "text") probe(x.value, "string"); else probe(x.value, "number");
  if ("text" !== x.kind) probe(x.value, "number"); else probe(x.value, "string");
}
function presence(x: { text: string } | { count: number }) {
  if ("text" in x) probe(x.text, "string"); else probe(x.count, "number");
}
function literal(x: string | number) {
  if (x === 1) probe(x, "one");
  if ("ready" === x) probe(x, "ready");
}
function openPresence(x: { [key: string]: number } | { count: string }) {
  if ("text" in x) probe(x.count, "number");
}
