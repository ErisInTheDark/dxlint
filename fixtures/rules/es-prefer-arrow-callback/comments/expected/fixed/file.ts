foo( /* keep */ ( /* parameter */ a) => /* body */ { return a; });
foo(async  /* keep */ () => { return 1; });
foo((
  ...args
) => // keep newline
{ return args; });
foo((() => { return this; }));
