((): undefined => {})() as undefined;
declare function exact(): 3;
let called = exact() as 3;
declare function nothing(): null;
let nullCall = nothing() as null;
let bareNull = null as null;
let bareUndefined = undefined as undefined;
let bareNumber = 3 as 3;
let bareBoolean = true as true;
