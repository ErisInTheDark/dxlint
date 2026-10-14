declare function choose<T = number, U = string>(): void;
choose<boolean>();
choose();
choose<boolean,>();
type Box<T = number> = { value: T };
type Nested = Box<Box>;
class C<T = number> {}
new C();
class D extends C {}
interface I<T = number> {}
class E implements I {}
choose</* keep */ number>();
choose<boolean, /* keep */ string>();
