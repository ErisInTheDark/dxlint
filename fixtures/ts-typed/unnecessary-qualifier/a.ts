namespace A {
  export type T = number;
  export const value = 3;
  const typed: A.T = 3;
  const read = A.value;
  namespace Nested {
    type T = string;
    const shadowed: A.T = 3;
    const outer = A.value;
  }
}
namespace Other {
  const external: A.T = 3;
}
namespace Outer {
  export namespace Inner {
    export const value = 1;
    const all = Outer.Inner.value;
    const withComment = Outer /* keep */ .Inner.value;
    function shadow(value: number) { return Outer.Inner.value; }
    const optional = Outer?.Inner.value;
    const computed = Outer["Inner"].value;
    const line = Outer.Inner // keep
      .value;
  }
}
const outside = Outer.Inner.value;
namespace Outer {
  export namespace Inner {
    function allShadowed(Inner: { value: number }, value: number) { return Outer.Inner.value; }
    function namespaceShadowed(Outer: { Inner: { value: number } }) { return Outer.Inner.value; }
  }
}
