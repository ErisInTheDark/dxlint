export class Base<T> {
  inherited(this: T): void {}
  static staticMethod(): void {}
}
export class ForeignConsole {
  log(): void {}
}
export const console = new ForeignConsole();
