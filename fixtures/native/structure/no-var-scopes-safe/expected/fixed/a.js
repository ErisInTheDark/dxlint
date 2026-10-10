for (let a of list) { use(a); }
for (let b in object) { use(b); }
for (let i = 0; i < 2; i++) { use(i); }
for (let item of list) { let c = 1; use(c); }
function caught() { try {} catch (e) { let x = 1; use(x); } }
function staticScope() { try {} catch (e) { class C { static { let e = 1; use(e); } } } }
function nested() { if (condition) { let value = 1; use(value); } }
function comments() { { let /* keep */ value = 1; use(value); } }
