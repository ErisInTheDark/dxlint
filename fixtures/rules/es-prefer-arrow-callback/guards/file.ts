foo(function() { return this; });
foo(function(a, a) { return a; });
foo(function(this: Thing, a) { return a; });
foo((other || function() { return this; }).bind(this));
foo((function() { return this; }.bind)(this));
foo(function() { return this; }.bind(/* keep */ this));
foo(function recur() { return recur(); });
foo(function() { return arguments[0]; });
foo(function*() { yield 1; });
