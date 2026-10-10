class C { delay = 13; }
declare const x: C;
if (typeof x.delay === "boolean") {
  x.delay = x.delay === true ? 13 : 0;
}
export {};
