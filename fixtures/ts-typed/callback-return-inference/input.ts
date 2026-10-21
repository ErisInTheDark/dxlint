interface Thenable<T> { then(onfulfilled: (value: T) => void): void }
declare const thenable: Thenable<number>;

declare function make<R>(f: () => R): R;
if (make(() => thenable)) {}

declare function apply<T, R>(x: T, f: (t: T) => R): R;
if (apply(1, t => thenable)) {}

declare function keep<T>(x: T, f: (t: T) => void): T;
if (keep(thenable, t => {})) {}

interface List<T> { map<U>(f: (value: T) => U): List<U>; first(): T }
declare const list: List<number>;
if (list.map(value => thenable).first()) {}
if (list.map(value => 1).first()) {}
