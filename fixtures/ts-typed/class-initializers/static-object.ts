class C { static defaults = { delay: 13 }; options = C.defaults; }
declare const x: C;
if (typeof x.options.delay === "boolean") {
  x.options.delay = x.options.delay === true ? 13 : 0;
}
export {};
