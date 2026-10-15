interface SymbolConstructor {
  readonly iterator: unique symbol;
  readonly asyncIterator: unique symbol;
}
declare const Symbol: SymbolConstructor;
interface Generator<T = unknown, TReturn = any, TNext = any> {
  [Symbol.iterator](): Generator<T, TReturn, TNext>;
  next(...args: [] | [TNext]): unknown;
}
