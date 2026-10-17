declare function custom<T = Options>(): T;
declare function scalar<T = number>(): T;
custom<Empty>();
scalar();
declare function mutable<T = Mutable>(): T;
mutable<Readonly<Mutable>>();
declare function choose<T>(a: [T, boolean], b: [T, boolean], or: T): T;
declare const a: Cardinal, b: Cardinal;
choose<Direction>([a, true], [b, true], Direction.None);
