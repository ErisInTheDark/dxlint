interface Number { toFixed(fractionDigits?: number): string }
interface String { charAt(pos: number): string }
interface Boolean { valueOf(): boolean }
interface BigInt { valueOf(): bigint }
interface Symbol { valueOf(): symbol }
interface Promise<T> { then(onfulfilled: (value: T) => unknown): void }
declare const Promise: { resolve<T>(value: T): Promise<Awaited<T>> };
type Awaited<T> = T extends null | undefined ? T
  : T extends object & { then(onfulfilled: infer F, ...args: infer _): any }
    ? F extends (value: infer V, ...args: infer _) => any ? Awaited<V> : never
    : T;
