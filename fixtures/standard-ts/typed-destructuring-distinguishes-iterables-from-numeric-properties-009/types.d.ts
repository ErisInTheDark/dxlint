interface Array<T> { [index: number]: T; }
interface ReadonlyArray<T> { readonly [index: number]: T; }
interface SymbolConstructor { readonly iterator: unique symbol; }
declare const Symbol: SymbolConstructor;
