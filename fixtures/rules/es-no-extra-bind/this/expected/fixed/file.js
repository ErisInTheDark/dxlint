var uses = function() { return this; }.bind(receiver);
var arrow = (() => this);
var nested = function() { function inner() { return this; } };
var captures = function() { return () => this; }.bind(receiver);
var fields = function() { class C { field = this.value; } };
var nesting = function() { var inner = (() => 1); };
