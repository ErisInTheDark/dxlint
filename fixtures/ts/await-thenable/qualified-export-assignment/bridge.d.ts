declare namespace Outer {
  namespace Public {
    interface Result { size: number }
    interface Bridge {
      read(): Result;
      pending(): Promise<Result>;
    }
  }
}
declare module "bridge" {
  export=Outer.Public;
}
