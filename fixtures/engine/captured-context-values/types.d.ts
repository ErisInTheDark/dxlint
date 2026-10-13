interface SymbolConstructor { readonly iterator: unique symbol; }
declare const Symbol: SymbolConstructor;
interface Iterator<T> { next(): { value: T; done: boolean }; }
interface Array<T> { [index: number]: T; [Symbol.iterator](): Iterator<T>; }
interface ReadonlyArray<T> { readonly [index: number]: T; [Symbol.iterator](): Iterator<T>; }
