const a = new Foo/* first *//* second */< /* inner */ string> /* last */();
const b = new
  Foo/* move */<string>
  ();
const c: // keep the line break
  Foo<string> = new Foo();
const d = new Foo<string /* trailing */,>();
const e /* outside */ = new Foo<string>();
