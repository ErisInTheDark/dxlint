namespace N {

  export const x = 1

}
declare module "m" {

  export const x: number

}
namespace A.B {

  export const x = 1

}
namespace Outer {

  namespace Inner {

    export const x = 1

  }

}
