const a = new Foo/* before */ </* inner */ string, /* middle */ number> /* after */();
const b = new
  Foo
  <string>
  ();
const c = new Foo<string /* trailing */,>();
const d /* binding */ = new Foo<string>();
