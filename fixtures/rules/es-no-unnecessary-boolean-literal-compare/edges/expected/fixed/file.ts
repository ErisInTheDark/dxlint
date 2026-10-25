// Unicode prefix: é
declare const a: boolean;
declare const b: boolean;
declare const c: boolean;
(a || b) && c;
!a;
!(typeof a === "boolean");
function test<T extends boolean>(value: T) { return value; }
function unknown<T>(value: T) { return value === true; }
a /* keep */ === true;
