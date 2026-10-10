declare function clone<T>(value: T): T;
declare const input: { delay: number };
class C { options = clone(input); }
declare const x: C;
if (typeof x.options.delay === "boolean") {
  x.options.delay = x.options.delay === true ? 13 : 0;
}
export {};
