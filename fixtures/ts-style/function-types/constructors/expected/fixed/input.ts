type Α = new (x: number) => Foo;
type B = new <T>(x: T) => Foo<T>;
type C = (new () => Foo) | undefined;
