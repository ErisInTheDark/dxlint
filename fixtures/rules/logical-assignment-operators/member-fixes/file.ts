declare const a: { b?: number; c?: number; d?: number };
declare let key: string;
declare const o: { [key: string]: number };

a.b || (a.b = 1);
a.c || (a.c = a.d);
o[key] || (o[key] = 1);

if (a.b) a.b = 1;
if (o[key]) o[key] = 1;

function base() { return { b: 0 }; }
base().b || (base().b = 1);
