class C { options = { delay: 13 }; }
declare const x: C;
if (typeof x.options.delay === "boolean") {
  x.options.delay = x.options.delay === true ? 13 : 0;
}
export {};
