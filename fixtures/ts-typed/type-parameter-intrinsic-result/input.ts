declare function unused<T>(): unknown;
declare function once<T>(value: T): unknown;
declare function twice<T>(left: T, right: T): unknown;
declare function retained<T>(value: T): T;
type Hidden<T> = Missing<T>;
declare function opaque<T>(value: T): Hidden<T>;
declare function parameterUnknown<T>(value: unknown): T;
