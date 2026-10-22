type Named = { id: number };
declare const alias: Named;
alias;
declare const fn: (v: number) => void;
fn;
declare const generic: <T>(v: T) => T;
generic;
declare const optionalTuple: [string, number?];
optionalTuple;
declare const restTuple: [string, ...number[]];
restTuple;
type Fst<T> = T extends { then(onfulfilled: infer F, ...args: infer _): any } ? F : 0;
interface MyThenable<T> { then(onfulfilled: (value: T) => void): any; }
declare const thenableCallback: Fst<MyThenable<string>>;
thenableCallback;
