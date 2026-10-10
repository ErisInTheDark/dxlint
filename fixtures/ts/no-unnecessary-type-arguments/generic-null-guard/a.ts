interface Element { tag: string }
class C<T extends Element = Element> { constructor(x: T) {} }
function early<T extends Element>(x: T | null) {
  if (!x) return;
  return new C<T>(x);
}
function equal<T extends Element>(x: T | null) {
  if (x === null) return;
  return new C<T>(x);
}
function written<T extends Element>(x: T | null) {
  if (!x) return;
  x = null;
  return new C<T>(x);
}
function nested<T extends Element>(x: T | null) {
  if (!x) return;
  const child = () => new C<T>(x);
}
function unrelated<T extends Element>(x: T | null, y: T | null) {
  if (!y) return;
  return new C<T>(x);
}
