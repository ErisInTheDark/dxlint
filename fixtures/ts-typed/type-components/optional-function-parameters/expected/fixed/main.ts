namespace Case946 {
(a?: string  ) => {};
}
namespace Case959 {

type T = undefined;
(arg?:   string) => {};
      
}
namespace Case978 {

interface F {
  (a?: string  ): void;
}
      
}
namespace Case999 {
type fn = new (a?: string  ) => void;
}
namespace Case1012 {
function f(a?: string  ) {}
}
namespace Case1025 {
f = function (a?: string  ) {};
}
namespace Case1038 {
declare function f(a?: string  ): void;
}
namespace Case155 {
(a: string | undefined) => {};
}
const empty = (arg?:   undefined) => {};
function constrained<T extends undefined>(arg?: T | string) {}
function acceptsVoid(arg?: void | string) {}
