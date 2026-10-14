declare function outer<T>(value: T): <U>(input: U) => U;
declare function captured<T>(value: T): () => T;
declare function callback<T>(value: () => T): void;
declare function unrelated<T>(value: <U>(input: U) => U): T;
declare function shadow<T>(value: <T>(input: T) => T): T;
declare function member<T>(value: { run: () => T }): void;
declare function receiver<T>(value: (this: T) => void): void;
declare function returnedArray<T>(): () => T[];
declare function unknown<T>(value: () => Missing): T;
declare function construct<T>(value: new () => T): void;
function inferred<T>(value: T) { return (input: T) => input; }
function independent<T>(value: T) { return <U>(input: U): U => input; }
interface Array<T> { length: number; [index: number]: T }
interface Cycle { (): Cycle }
declare function cycle<T>(value: { next: Cycle }): T;
