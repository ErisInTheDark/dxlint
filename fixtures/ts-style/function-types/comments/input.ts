interface Α {
  // keep
  (): string;
}
export interface B {
  /** keep */
  (): string;
}
type C = { /* keep */ (s: string): number } | undefined;
interface D { (x: /* inside */ string): /* return */ number; }
interface E { (): void; // trailing
}
