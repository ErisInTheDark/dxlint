function first(...args: any[]): unknown {}
function second(...args: readonly any[]) {}
type Fn = (...args: Array<any>) => unknown;
type ReadonlyFn = (...args: ReadonlyArray<any>) => void;
function bare(...args: unknown[]) {}
function nested(...args: Promise<unknown>[]) {}
let values: unknown[];
