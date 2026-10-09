interface Foo {
  foo: ((bar: string) => one) & ((bar: number, baz: string) => two) & (() => three);
  [key]: (<T>(arg: T) => T) & (() => void),
}
type Bar = { other(): number; bar: (() => one) & ((x: string) => two); };
declare const value: { call: (() => one) & ((x: string) => two) };
