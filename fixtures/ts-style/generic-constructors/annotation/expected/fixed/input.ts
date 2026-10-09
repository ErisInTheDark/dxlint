const a: Foo<string> = new Foo();
const b: Map<string, number> = new Map  ();
const c: Foo<Array<Map<string, number>>,> = new Foo;
const d: Foo<string> = (new Foo(1));
const note = "😀"; const é: Foo<"é"> = new Foo();
class C {
  accessor a: Foo<string> = new Foo();
  [a + b]: Foo<number> = new Foo();
  definite!: Foo<string> = new Foo();
  optional?: Foo<string> = new Foo();
  constructor(public p: Foo<string> = new Foo()) {}
}
function f(a: Foo<string> = new Foo(), {b}: Foo<string> = new Foo(), [c]: Foo<string> = new Foo()) {}
const fn = function (a: Foo<string> = new Foo()) {};
class Nested { a: Foo<string> = new Foo(() => { const b: Foo<number> = new Foo(); }); }
