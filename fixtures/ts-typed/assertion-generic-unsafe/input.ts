function constrained<T extends string>(value: string) { return value as T; }
function constrainedObject<T extends { a: number }>(value: { a: number }) { return value as T; }
function unconstrained<T>(value: string) { return value as T; }
function arbitrary<T, U>(value: U) { return value as T; }
function extendsUnknown<T extends unknown>(value: string) { return value as T; }
function extendsAny<T extends any>(value: string) { return value as T; }
function down<T, U extends T>(value: T) { return value as U; }
function downKnown<T extends string, U extends T>(value: T) { return value as U; }
function narrow<T extends string>(value: T) { return value as "a"; }
function disjoint<T extends string>(value: number) { return value as T; }
function sourceUnknown<T>(value: T) { return value as string; }
function same<T>(value: T) { return value as T; }
function sameKnown<T extends string>(value: T) { return value as T; }
function up<T, U extends T>(value: U) { return value as T; }
function upKnown<T extends string, U extends T>(value: U) { return value as T; }
function wider<T extends string>(value: T) { return value as string | number; }
function unknownTarget<T>(value: T) { return value as unknown; }
function neverSource<T>(value: never) { return value as T; }
function neverTarget(value: string) { return value as never; }
