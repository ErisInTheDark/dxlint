const a = function() {}();
const b = function() {}["apply"](null, []);
const c = (function() {})();
const d = (function() {}());
if (function() {}()) {}
consume(function() {}());
const e = (() => 1)();
