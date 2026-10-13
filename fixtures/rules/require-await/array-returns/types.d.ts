declare const iterator: unique symbol;
interface Array<T> { [index: number]: T; [iterator](): unknown; length: number }
interface ReadonlyArray<T> { readonly [index: number]: T; [iterator](): unknown; readonly length: number }
