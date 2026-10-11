class C { options = { delay: true }; }
declare const x: C;
if (typeof x.options.delay === "boolean") {
  x.options.delay = x.options.delay ? false : true;
}
export {};
