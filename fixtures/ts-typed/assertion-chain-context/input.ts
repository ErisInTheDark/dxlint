declare const n: number;
const numeric = n as any as number;
declare function accept(value: number): void;
accept(42 as any as number);
accept(n as any);
declare const text: string;
const retained = text as unknown as number;
let literal = 3 as any as 3;
const duplicate = (n as number) as number;
const needed = (n as number) as unknown as string;
const widened: number | null = null as number | null;
interface Wide { name?: string; }
interface Narrow { name: string; }
declare const narrow: Narrow;
const satisfied = narrow as Wide satisfies Wide;
