type Α<T> = (this: string) => T;
function foo(a: (s: string) => number, b: (() => void) | undefined) {}
type C = {} | (() => void);
type D = {} & (() => void);
type E = (() => void)[];
type F = (() => void);
