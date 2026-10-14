interface Array<T> { length: number; [index: number]: T }
interface Array<T> { first: T }
interface Array<T> { last: T }
interface ReadonlyArray<T> { readonly length: number; readonly [index: number]: T }
interface ReadonlyArray<T> { readonly first: T }
interface ReadonlyArray<T> { readonly last: T }
