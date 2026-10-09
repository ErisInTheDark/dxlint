namespace Case946 {
(a?: string | undefined) => {};
}
namespace Case959 {

type T = undefined;
(arg?: T | string) => {};
      
}
namespace Case978 {

interface F {
  (a?: string | undefined): void;
}
      
}
namespace Case999 {
type fn = new (a?: string | undefined) => void;
}
namespace Case1012 {
function f(a?: string | undefined) {}
}
namespace Case1025 {
f = function (a?: string | undefined) {};
}
namespace Case1038 {
declare function f(a?: string | undefined): void;
}
namespace Case155 {
(a: string | undefined) => {};
}
const empty = (arg?: undefined | undefined) => {};
function constrained<T extends undefined>(arg?: T | string) {}
function acceptsVoid(arg?: void | string) {}
