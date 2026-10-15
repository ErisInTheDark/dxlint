declare const x: { a: { b: number } | null } | null;
x.a;
x.a && x.a.b;
