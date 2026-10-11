type Foo = import('x')[];
type Bar = boolean | Bar & Baz;
type Baz = (string | number)[];
type Fn = ((a: string) => number)[];
class A { accessor [(foo)] = 1 }
