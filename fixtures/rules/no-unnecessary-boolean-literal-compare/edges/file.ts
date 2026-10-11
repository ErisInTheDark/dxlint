// Unicode prefix: é
declare const a: boolean;
declare const b: boolean;
declare const c: boolean;
(a || b) === true && c;
!(a !== false);
typeof a === "boolean" === false;
function test<T extends boolean>(value: T) { return value === true; }
function unknown<T>(value: T) { return value === true; }
a /* keep */ === true;
