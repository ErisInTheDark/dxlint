abstract class Α {
  x = 2; 'foo foo' = 3;
  abstract y: string;
  static readonly z = 'public';
  constructor(readonly value: string, public plain: string) {}
  abstract ['computed-method'](): string;
  get foo() { return this.x; }
  set foo(value: number) { this.x = value; }
  @public method() {}
  @public field: string;
}
