const obj = { foo: 1 };
let other: typeof obj;
const {foo} = other = obj;
const {call} = () => null;
const fooTyped: number = obj.foo;
