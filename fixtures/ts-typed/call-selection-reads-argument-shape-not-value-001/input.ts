declare function rest(a: any, two: unknown, ...more: any[]): asserts two;
rest(3 as any, "" as string | null, "more", "args", "afterwards");
declare function two(a: number, b: unknown): asserts b;
declare const s: string | null;
two(missing, s);
