const a = new Foo<string>();
const b = new Map<string, number> ();
const c = new Foo<Array<Map<string, number>>,>();
const d = (new Foo<string>(1));
const note = "😀"; const é = new Foo<"é">();
class C {
  accessor a = new Foo<string>();
  [a + b] = new Foo<number>();
  definite! = new Foo<string>();
  optional? = new Foo<string>();
  constructor(public p = new Foo<string>()) {}
}
function f(a = new Foo<string>(), {b} = new Foo<string>(), [c] = new Foo<string>()) {}
const fn = function (a = new Foo<string>()) {};
class Nested { a = new Foo<string>(() => { const b = new Foo<number>(); }); }
