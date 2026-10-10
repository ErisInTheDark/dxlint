const obj = { foo: 1 };
let other: typeof obj;
const foo = (other = obj).foo;
const call = (() => null).call;
const fooTyped: number = obj.foo;
