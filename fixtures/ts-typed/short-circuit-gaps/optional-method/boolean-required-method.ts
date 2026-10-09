interface C { f(): boolean } declare const x: C; export const result = x.f() === false;
