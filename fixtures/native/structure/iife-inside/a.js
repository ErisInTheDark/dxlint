const a = function() {}();
const b = (function() {}());
const c = ((function() {}()));
const d = (function() {} /* keep */ ());
if (function() {}()) {}
consume(function() {}());
const e = new (function() {}())();
const f = (function() {}.call(null));
const g = (function() {})();
const h = (() => 1)();
