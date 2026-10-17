((): undefined => {})();
declare function exact(): 3;
let called = exact();
declare function nothing(): null;
let nullCall = nothing();
let bareNull = null as null;
let bareUndefined = undefined as undefined;
let bareNumber = 3 as 3;
let bareBoolean = true as true;
