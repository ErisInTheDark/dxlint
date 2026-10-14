declare function choose<T = number, U = string>(): void;
choose<boolean, string>();
choose<number>();
choose<boolean, string,>();
type Box<T = number> = { value: T };
type Nested = Box<Box<number>>;
class C<T = number> {}
new C<number>();
class D extends C<number> {}
interface I<T = number> {}
class E implements I<number> {}
choose</* keep */ number>();
choose<boolean, /* keep */ string>();
