function inspect() {
  class C {
    readonly marker: true = true;
    self(): C { return this; }
  }
  const instance = new C();
  instance.self().marker;
}
