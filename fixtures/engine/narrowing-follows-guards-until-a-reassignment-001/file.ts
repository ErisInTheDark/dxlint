interface Foo { a: number }
declare const b: string | number;
declare const maybe: Foo | null;
function f(x: Foo | undefined, y: string | undefined) {
  if (x) { x; }
  if (typeof b === "string") { b; } else { b; }
  if (!x) return;
  x;
  if (y) { y = undefined; y; }
  maybe && maybe;
}
