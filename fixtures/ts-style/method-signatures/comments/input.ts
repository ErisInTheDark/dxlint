interface Test {
  safe</* type */ T>(/* argument */ x: T): T;
  empty(/* keep */): void;
  header /* keep */ (x: number): string;
  result(): /* keep */ string;
  overload(): one;
  /* keep the overload note */
  overload(x: string): two;
}
