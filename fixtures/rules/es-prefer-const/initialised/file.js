// Unicode before byte spans: café
let a = 1;
let b = 2, c = 3;
let {d: {e, f}} = obj;
let [g, , ...h] = values;
let /*keep*/ i = 4;
let outer = () => { let inner = 5; inner++; };
let shadow = 0; { let shadow = 1; use(shadow); } shadow++;
let {callback = (value) => value} = obj;
let {other = () => { let local = 1; local++; }} = obj;
