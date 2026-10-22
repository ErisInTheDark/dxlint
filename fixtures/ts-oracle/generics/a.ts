function identity<T>(value: T) {
  value;
  return value;
}
identity(1);
identity("s");
interface Box<T> { value: T; }
declare const box: Box<number>;
box;
box.value;
declare const boxes: Box<number>[];
boxes;
declare const nestedBox: Box<Box<string>>;
nestedBox;
type Unwrap<T> = T extends Box<infer V> ? V : never;
declare const u1: Unwrap<Box<string>>;
u1;
declare const u2: Unwrap<number>;
u2;
interface Pair<A, B = A> { first: A; second: B; }
declare const pair: Pair<number>;
pair;
type KeyOf<T> = keyof T;
declare const key: KeyOf<Box<number>>;
key;
function first<T>(values: T[]): T {
  return values[0];
}
first([1, 2]);
