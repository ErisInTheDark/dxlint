const a = function() {}();
const b = (function() {})();
const c = ((function() {}))();
const d = (function() {} /* keep */) /* tail */ ();
const e = function() {}.call(null);
if (function() {}()) {}
consume(function() {}());
const f = (function() {}());
const g = (() => 1)();
