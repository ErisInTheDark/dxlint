function foo(x: any) {
  x['a'];
}
function bar(x: { a: number }, y: any) {
  x[y];
}
function baz(x: { a: number }, y: any) {
  x[y++];
}
function qux(x?: { a: number }, y: any) {
  x?.[y];
}
