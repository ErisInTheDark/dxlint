interface Array<T> { [index: number]: T; length: number }
interface ReadonlyArray<T> { readonly [index: number]: T; readonly length: number }
interface ReadonlyArray<T> { then(callback: (value: T) => unknown): unknown }
