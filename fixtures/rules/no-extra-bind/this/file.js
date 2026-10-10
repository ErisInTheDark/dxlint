var uses = function() { return this; }.bind(receiver);
var arrow = (() => this).bind(receiver);
var nested = function() { function inner() { return this; } }.bind(receiver);
var captures = function() { return () => this; }.bind(receiver);
var fields = function() { class C { field = this.value; } }.bind(receiver);
var nesting = function() { var inner = (() => 1).bind(receiver); }.bind(receiver);
