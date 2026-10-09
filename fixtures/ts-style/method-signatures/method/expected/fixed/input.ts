interface Test {
  f(a: string): number;
  ['f']?<T extends {}>(a?: T, b?: T): T;
  'f!'</* generic */ T>(/* param */ x: T /* tail */): T;
  comma(): void,
  implicit(): any
}
type T = { optional?<T>(x: T): T };
const note = "😀"; type É = { é(x: "é"): "é" };
interface Nested { outer(): { inner(): string }; other(): number; }
