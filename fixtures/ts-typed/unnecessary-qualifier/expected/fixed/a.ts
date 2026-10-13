namespace A {
  export type T = number;
  export const value = 3;
  const typed: A.T = 3;
  const read = value;
  namespace Nested {
    type T = string;
    const shadowed: A.T = 3;
    const outer = value;
  }
}
namespace Other {
  const external: A.T = 3;
}
namespace Outer {
  export namespace Inner {
    export const value = 1;
    const all = value;
    const withComment = /* keep */ value;
    function shadow(value: number) { return Inner.value; }
    const optional = Outer?.Inner.value;
    const computed = Outer["Inner"].value;
    const line = // keep
value;
  }
}
const outside = Outer.Inner.value;
namespace Outer {
  export namespace Inner {
    function allShadowed(Inner: { value: number }, value: number) { return Outer.Inner.value; }
    function namespaceShadowed(Outer: { Inner: { value: number } }) { return Outer.Inner.value; }
  }
}
