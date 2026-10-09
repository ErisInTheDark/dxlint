const a: Foo</* inner */ string, /* middle */ number> = new Foo/* before */  /* after */();
const b: Foo<string> = new
  Foo
  
  ();
const c: Foo<string /* trailing */,> = new Foo();
const d: Foo<string> /* binding */ = new Foo();
