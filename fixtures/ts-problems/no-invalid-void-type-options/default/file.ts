declare function f<T>(): T;
declare const generic: Promise<void>;
type Defaulted<T = void> = T;
f<void>();
let plain: void;
type Union = void | string;
function withThis(this: void) {}
