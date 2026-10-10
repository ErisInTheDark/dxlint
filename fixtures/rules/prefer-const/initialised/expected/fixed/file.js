// Unicode before byte spans: café
const a = 1;
const b = 2, c = 3;
const {d: {e, f}} = obj;
const [g, , ...h] = values;
const /*keep*/ i = 4;
const outer = () => { let inner = 5; inner++; };
let shadow = 0; { const shadow = 1; use(shadow); } shadow++;
const {callback = (value) => value} = obj;
const {other = () => { let local = 1; local++; }} = obj;
