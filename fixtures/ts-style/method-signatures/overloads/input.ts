interface Foo {
  foo(bar: string): one;
  foo(bar: number, baz: string): two;
  foo(): three;
  [key]<T>(arg: T): T;
  [key](): void,
}
type Bar = { bar(): one; other(): number; bar(x: string): two; };
declare const value: { call(): one; call(x: string): two };
