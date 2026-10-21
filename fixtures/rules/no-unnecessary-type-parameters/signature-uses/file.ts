function printProperty<T, K extends keyof T>(obj: T, key: K) {
  console.log(obj[key]);
}

declare function take<T, U extends T>(param: U): U;

declare function unused<T>(): void;

function getLength<T extends { length: number }>(x: T) {
  return x.length;
}

declare function join<T extends () => void>(els: T[]): void;

const asserted = <T extends string>(value: T) => value as { [Key in "a" as T]: 0 };

type Query = <T>(input: T) => typeof input;

function getProperty<T, K extends keyof T>(obj: T, key: K) {
  return obj[key];
}

function id<T>(x: T) {
  return x;
}

declare function fetchJson<T>(url: string): Promise<T>;

declare function makeMap<K, V>(): Map<K, V>;

class ClassyArray<T> {
  arr: T[] = [];
}
