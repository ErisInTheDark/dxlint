namespace A {
  export namespace B.C {
    export type D = number;
    const x: D = 3;
    namespace Nested {
      type D = string;
      const shadowed: B.C.D = 3;
    }
  }
}
const outside: A.B.C.D = 3;
