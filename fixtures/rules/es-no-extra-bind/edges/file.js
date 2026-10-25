var before = function() {} /* keep */ .bind(receiver);
var line = function() {} // keep
 .bind(receiver);
var after = function() {}.bind(receiver)/* keep */;
var wrappers = ((function() {}).bind)(receiver);
var optionalCall = function() {}.bind?.(receiver);
var optionalMember = function() {}?.bind(receiver);
