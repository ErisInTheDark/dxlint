commented: { /* break commented; */ work(); }
quoted: { consume("continue quoted;"); }
template: { consume(`break template;`); }
regexWords: { consume(/break regexWords/); }
used: { break /* keep */ used; }
loop: while (condition) { continue /* keep */ loop; }
outer: { inner: { break outer; } }
same: { function nested() { same: { break same; } } }
dollars$: { break dollars$; }
café: { break café; }
esc\u0061ped: { break escaped; }
staticOuter: { class C { static { staticOuter: { break staticOuter; } } } }
arrowOuter: { const nested = () => { arrowOuter: { break arrowOuter; } }; }
methodOuter: { const object = { method() { methodOuter: { break methodOuter; } } }; }
