declare const input: { delay: number };
class Box<T> { value: T; constructor(value: T) { this.value = value; } }
class C { options = new Box(input); }
declare const x: C;
if (typeof x.options.value.delay === "boolean") {
  x.options.value.delay = x.options.value.delay === true ? 13 : 0;
}
export {};
