function asserted(this: object, a: number, b: unknown): asserts b;
function asserted(a: bigint, b: unknown): asserts b;
function asserted(...args: any[]) { throw 0; }
function open(a: bigint, b: unknown): asserts b;
function open(...args: any[]): void;
function open(...args: any[]) { throw 0; }
function alone(value: unknown): asserts value { throw 0; }
declare const nullable: string | null;
open(3 as any, nullable);
asserted(3 as any, nullable);
declare const other: string | null;
alone(other);
export {};
