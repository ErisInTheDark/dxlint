interface Array<T> {
  [key: number]: T;
  length: number;
}
interface ReadonlyArray<T> {
  readonly [key: number]: T;
  readonly length: number;
}
