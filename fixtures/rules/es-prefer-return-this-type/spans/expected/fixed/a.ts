class Foo {
  f(): this {
    return this;
  }
}
class Bar {
  g(): this | undefined {
    return this;
  }
}
class Baz {
  h = (): this => this;
}
