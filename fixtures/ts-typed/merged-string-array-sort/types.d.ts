interface Array<T> { [key: number]: T; length: number; }
interface Array<T> { sort(compare?: (a: T, b: T) => number): this; }
