var fx = function (i = 0) { return i < 5 ? fx(i + 1) : i; }; fx();
var foo = () => foo();
function captured() { for (let i of list) { var foo = () => foo(); save(foo); } }
var result = (function () { return result; })();
function early() { use(foo); var foo = () => foo(); }
function hoisted() { foo(); var value = 1; function foo() { return value; } }
function duplicate() { var foo = () => foo(); var foo; }
