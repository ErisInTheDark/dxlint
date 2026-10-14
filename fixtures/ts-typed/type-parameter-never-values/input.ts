declare function once<T>(value: T): never;
declare function absent<T>(): never;
declare function retained<T>(value: T): T;
type Hidden<T> = Missing<T>;
declare function opaque<T>(value: Hidden<T>): never;
