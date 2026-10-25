declare const a: { b?: number; c?: number; d?: number };
declare let key: string;
declare const o: { [key: string]: number };

a.b ||= 1;
a.c ||= a.d;
o[key] ||= 1;

a.b &&= 1;
o[key] &&= 1;

function base() { return { b: 0 }; }
base().b || (base().b = 1);
