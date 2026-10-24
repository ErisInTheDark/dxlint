interface P { a: string; b?: number; readonly c: boolean }
declare const k: keyof P;
k;
declare const v: P[keyof P];
v;
interface A { a: 1 }
interface B { b: 2 }
declare const u: keyof (A | B);
u;
declare const i: keyof (A & B);
i;
enum E { X, Y }
declare const e: keyof typeof E;
e;
declare const s: { [k: string]: number };
declare const sk: keyof typeof s;
sk;
declare const q: P["a" | "c"];
q;
export class Box<T extends keyof P> { t!: T }
declare const box: Box<"a">;
box.t;
