declare const n: number;
const numeric = n;
declare function accept(value: number): void;
accept(42);
accept(n);
declare const text: string;
const retained = text as unknown as number;
let literal = 3 as any as 3;
const duplicate = n;
const needed = (n) as unknown as string;
const widened: number | null = null as number | null;
interface Wide { name?: string; }
interface Narrow { name: string; }
declare const narrow: Narrow;
const satisfied = narrow as Wide satisfies Wide;
