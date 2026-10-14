class Foo {
  f(): this { return this; }
  g = function (): this { return this; };
  h = (): this => this;
  other(): Foo { return new Foo(); }
}
