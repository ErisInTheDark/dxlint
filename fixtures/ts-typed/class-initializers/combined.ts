const divisor = 4;
const defaultDelay = 200 / divisor;
declare function clone<T>(value: T): T;
class C { static defaults = { delay: defaultDelay }; options = clone(C.defaults); }
declare const x: C;
if (typeof x.options.delay === "boolean") {
  x.options.delay = x.options.delay === true ? 13 : 0;
}
export {};
