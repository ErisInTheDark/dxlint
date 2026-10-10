// Unicode prefix: é
var plain = function() { return "é"; }.bind(receiver);
var computed = function() {}["bind"](this);
var templated = function() {}[`bind`](0);
var parenthesised = (function() {}.bind ) (receiver);
var optional = (function() {}?.["bind"] ) ?.(receiver);
var argument = function() {}.bind((function() {}));
