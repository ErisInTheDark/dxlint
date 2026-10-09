enum E { A, B }
enum S { A = "a" }
declare function make<T = number>(): T;
declare function same<T>(x: T): T;
class Box<T = string> {}
make<E>();
make<S>();
make<string>();
same<number>(1);
let b: Box<string>;
