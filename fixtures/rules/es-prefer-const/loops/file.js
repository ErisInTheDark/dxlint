for (let a in obj) use(a);
for (let b of values) use(b);
for (let [c, d] of values) use(c, d);
for (let e of values) { e++; }
for (let i = 0; i < 3; i++) use(i);
class C { static { let value = 1; use(value); } }
