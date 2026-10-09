const a: /* first */ Foo /* second */ < /* inner */ string> = new Foo /* last */();
const b: Foo /* move */ <string> = new
  Foo
  ();
const c: // keep the line break
  Foo<string> = new Foo();
const d: Foo<string /* trailing */,> = new Foo();
const e: Foo<string> /* outside */ = new Foo();
