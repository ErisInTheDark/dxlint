class Foo {
  f(): Foo {
    return this;
  }
}
class Bar {
  g(): Bar | undefined {
    return this;
  }
}
class Baz {
  h = (): Baz => this;
}
