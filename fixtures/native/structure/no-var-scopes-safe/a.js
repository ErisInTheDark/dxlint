for (var a of list) { use(a); }
for (var b in object) { use(b); }
for (var i = 0; i < 2; i++) { use(i); }
for (let item of list) { var c = 1; use(c); }
function caught() { try {} catch (e) { var x = 1; use(x); } }
function staticScope() { try {} catch (e) { class C { static { var e = 1; use(e); } } } }
function nested() { if (condition) { var value = 1; use(value); } }
function comments() { { var /* keep */ value = 1; use(value); } }
