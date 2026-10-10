{ /* break commented; */ work(); }
{ consume("continue quoted;"); }
{ consume(`break template;`); }
{ consume(/break regexWords/); }
used: { break /* keep */ used; }
loop: while (condition) { continue /* keep */ loop; }
outer: { { break outer; } }
{ function nested() { same: { break same; } } }
dollars$: { break dollars$; }
café: { break café; }
esc\u0061ped: { break escaped; }
{ class C { static { staticOuter: { break staticOuter; } } } }
{ const nested = () => { arrowOuter: { break arrowOuter; } }; }
{ const object = { method() { methodOuter: { break methodOuter; } } }; }
