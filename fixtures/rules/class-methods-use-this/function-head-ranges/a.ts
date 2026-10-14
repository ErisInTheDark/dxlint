class Example {
  plain () {}
  get value() { return 1; }
  set value(input: number) {}
  *items<T>() {}
  callback = function named() {};
  arrow = () => {};
  @decorate
  decorated<T>() {}
  used() { return this.value; }
  static independent() {}
  exempt() {}
  nested() { return () => this.value; }
  ownFunction() { return function() { return this; }; }
  accessor factory = () => {};
}
