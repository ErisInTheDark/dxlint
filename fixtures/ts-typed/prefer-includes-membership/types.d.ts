interface Array<T> {
  indexOf(value: T, fromIndex?: number): number;
  includes(value: T, fromIndex?: number): boolean;
}
interface Compatible {
  indexOf(value: string): number;
  includes(value: string): boolean;
}
interface Different {
  indexOf(value: string, fromIndex?: number): number;
  includes(value: string, fromIndex: number): boolean;
}
