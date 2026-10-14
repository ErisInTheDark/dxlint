namespace A {
  export type T = number;
  const typed: A.T = 1;
  namespace Nested {
    type T = string;
    const shadowed: A.T = 1;
  }
}
const outside: A.T = 1;
enum E {
  B,
  C = E.B,
}
namespace Foo {
  export enum F {
    B,
    C = Foo.F.B,
  }
}
