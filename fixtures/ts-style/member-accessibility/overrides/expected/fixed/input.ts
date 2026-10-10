class Α {
  x = 1;
  method() {}
  public ignored() {}
  public get value() { return this.x; }
  constructor(readonly stored: string, public plain: string) {}
}
