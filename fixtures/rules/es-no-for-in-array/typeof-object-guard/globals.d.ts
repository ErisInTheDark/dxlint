// What makes strings and arrays array-like, without TypeScript's libraries.
interface String { readonly [index: number]: string; readonly length: number }
interface Array<T> { [index: number]: T; length: number }
