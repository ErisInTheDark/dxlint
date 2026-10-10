abstract class Α {
  public x = 2; public 'foo foo' = 3;
  public abstract y: string;
  public static readonly z = 'public';
  public constructor(public readonly value: string, public plain: string) {}
  public abstract ['computed-method'](): string;
  public get foo() { return this.x; }
  public set foo(value: number) { this.x = value; }
  @public public method() {}
  @public public field: string;
}
