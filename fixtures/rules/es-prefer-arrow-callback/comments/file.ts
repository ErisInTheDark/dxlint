foo(function /* keep */ named( /* parameter */ a) /* body */ { return a; });
foo(async function /* keep */ () { return 1; });
foo(function(
  ...args
) // keep newline
{ return args; });
foo((function() { return this; }).bind(this));
