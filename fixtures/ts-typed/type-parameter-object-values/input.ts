declare function get<T extends object>(): T;
declare function take<T extends object>(value: T): void;
declare function retained<T extends object>(value: T): T;
declare function structural<T>(value: { entry: T }): void;
type Hidden<T> = Missing<T>;
declare function opaque<T>(value: Hidden<T>): void;
declare function objectOnly<T>(value: object): void;
