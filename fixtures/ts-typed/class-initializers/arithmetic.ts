class C { delay = 200 / 4; }
declare const x: C;
if (typeof x.delay === "boolean") {
  x.delay = x.delay === true ? 13 : 0;
}
export {};
