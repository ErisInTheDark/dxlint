for (const a in obj) use(a);
for (const b of values) use(b);
for (const [c, d] of values) use(c, d);
for (let e of values) { e++; }
for (let i = 0; i < 3; i++) use(i);
class C { static { const value = 1; use(value); } }
