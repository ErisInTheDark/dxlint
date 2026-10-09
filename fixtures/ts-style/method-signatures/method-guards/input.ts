interface Test {
  readonly f: (x: number) => string;
  readonly ['f']?: <T>(x?: T) => T;
  get f(): number;
  set f(value: number): void;
  construct: new () => Foo;
  abstractConstruct: abstract new () => Foo;
}
