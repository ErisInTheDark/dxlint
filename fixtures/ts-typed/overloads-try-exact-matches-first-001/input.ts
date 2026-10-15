declare function assert(this: object, a: number, b: unknown): asserts b;
declare function assert(a: bigint, b: unknown): asserts b;
declare function assert(...args: any[]): void;
declare const s: string | null;
assert(3 as any, s);
assert(3n, s);
