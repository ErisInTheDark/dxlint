interface Test {
  get f(): number;
  set f(value: number): void;
  self(): this;
  nested(): Promise<this | undefined>;
  overload(): this;
  overload(): Promise<this>;
}
namespace Example { export interface T { f(): void; } }
declare global { namespace Example { interface T { f(): void; f(x: string): void; } } }
