declare function probe(value: unknown, expected: string): void;
declare function source(): string | null;
function written(x: string | null) {
  if (!x) return;
  probe(x, "string");
  x = null;
  probe(x, "null");
}
function unrelated(x: string | null, y: string | null) {
  if (!x) return;
  y = null;
  probe(x, "string");
}
function shadow(x: string | null) {
  if (x) { let x: string | null = source(); probe(x, "string-null"); }
}
function mutable(x: string | null) {
  if (x) { const deferred = () => probe(x, "string"); }
}
function immutable() {
  const x: string | null = source();
  if (x) {
    const deferred = () => probe(x, "string");
    const nested = () => () => probe(x, "string");
  }
}
function property(x: { value: string | null }) {
  if (x.value) {
    probe(x.value, "string");
    const deferred = () => probe(x.value, "string-null");
    x.value = null;
    probe(x.value, "null");
  }
}
function loops(x: string | null) {
  while (x) { probe(x, "string"); x = null; probe(x, "null"); }
  for (; x;) { probe(x, "string"); x = null; }
}
