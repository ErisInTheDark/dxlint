class Foo {
  f(): Foo { return this; }
  g = function (): Foo { return this; };
  h = (): Foo => this;
  other(): Foo { return new Foo(); }
}
