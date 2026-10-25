interface Array<T> { [index: number]: T; length: number }
interface Array<T> { then(callback: (value: T) => unknown): unknown }
interface ReadonlyArray<T> { readonly [index: number]: T; readonly length: number }
