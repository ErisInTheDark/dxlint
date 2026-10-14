if (missing) {}
declare const typed: MissingType;
if (typed) {}
declare const opaque: unknown;
if (opaque) {}
const untyped = argument => !argument;
declare const array: string[];
array.some(() => missing);
