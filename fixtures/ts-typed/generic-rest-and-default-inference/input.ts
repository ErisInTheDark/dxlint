interface Thenable<T> { then(onfulfilled: (value: T) => void): void }
declare const thenable: Thenable<number>;

declare function pack<A extends unknown[]>(...args: A): A;
const packed = pack(thenable, 1);
if (packed[0]) {}
if (packed[1]) {}

declare function fallback<T = Thenable<number>>(): T;
const defaulted = fallback();
if (defaulted) {}
