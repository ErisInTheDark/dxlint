declare const source: unknown;
const receiving: unknown = source!;
declare function accept(value: unknown): void;
accept(source!);
declare function opaque(value: any): void;
opaque(source!);
declare function required(value: number): void;
required(source!);
source!;
function generic<T>() { const receiving: T = source!; }
const broken: Missing = source!;
function returning(): unknown { return source!; }
