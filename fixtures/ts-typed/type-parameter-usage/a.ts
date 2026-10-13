function single<T>(value: T): void {}
function identity<T>(value: T) { return value; }
function opaqueObject<T>(value: T) { return { read: () => value }; }
function tuple<T>(value: T) { return [value] as const; }
function mutable<T>(value: T) { return [value]; }
function never<T>(): void {}
function arrayInput<T>(value: T[]): void {}
declare function mutableOutput<T>(): T[];
declare function readonlyOutput<T>(): readonly T[];
class One<T> { value: T; }
class Two<T> { first: T; second: T; }
class ClassArray<T> { values: T[]; }
class Empty<T> {}
class Opaque<T> { read(): { value: T }; }
interface Box<T> { value: T; }
function wrapper<T>(value: Box<T>): void {}
class Inherited<T> extends Two<T> {}
declare function receiver<T>(this: T): void;
declare function templateWrapper<T extends string>(): Promise<`a${T}b`>;
declare function makeMutable<T>(input: T): MakeMutable<T>;
declare function makeMutable2<T>(input: T): MakeMutable<typeof input>;
declare function directTemplate<T extends string>(): `a${T}b`;
declare function scalar<T extends string>(value: T): void;
declare function inputArray<T extends string>(value: T[]): void;
declare function readOnlyResult<T extends string>(): readonly T[];
declare function mutableResult<T extends string>(): T[];
declare function two<T extends string>(value: T): T;
