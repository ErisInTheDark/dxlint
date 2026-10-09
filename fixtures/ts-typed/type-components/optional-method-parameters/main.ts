namespace Case1051 {

declare class bb {
  f(a?: string | undefined): void;
}
      
}
namespace Case1072 {

interface ee {
  f(a?: string | undefined): void;
}
      
}
namespace Case1093 {

interface ee {
  new (a?: string | undefined): void;
}
      
}
namespace Case1114 {
type fn = (a?: string | undefined) => void;
}
namespace Case1127 {
type fn = (a?: string | (undefined | number)) => void;
}
namespace Case1140 {
type fn = (a?: (undefined | number) | string) => void;
}
namespace Case1153 {

abstract class cc {
  abstract f(a?: string | undefined): void;
}
      
}
