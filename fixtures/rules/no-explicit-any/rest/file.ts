function first(...args: any[]): any {}
function second(...args: readonly any[]) {}
type Fn = (...args: Array<any>) => any;
type ReadonlyFn = (...args: ReadonlyArray<any>) => void;
function bare(...args: any) {}
function nested(...args: Promise<any>[]) {}
let values: any[];
