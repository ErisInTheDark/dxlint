declare function plain<T>(value: { name: string }): T;
declare function member<T>(value: { entry: T }): void;
declare function pair<T>(value: { first: T; second: T }): void;
declare function readonlyMember<T>(value: { readonly entry: T }): void;
declare function nested<T>(value: { outer: { entry: T } }): void;
declare function returned<T>(): { entry: T };
declare function independent<T>(value: { plain: number }): void;
type Hidden<T> = Missing<T>;
declare function opaque<T>(value: Hidden<T>): void;
declare function callable<T>(value: () => T): void;
interface Array<T> { length: number; [index: number]: T }
declare function arrayMember<T>(value: { entry: T[] }): void;
declare function returnedArray<T>(): { entry: T[] };
declare function indexMember<T>(value: { [key: string]: T }): void;
declare function nestedOpaque<T>(value: { entry: Hidden<T> }): void;
declare function bounded<T extends { entry: number }>(value: { entry: T }): void;
