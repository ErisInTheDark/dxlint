enum Fruit { Apple }
function direct<T extends Fruit[]>(): T {
  return [
    1
  ];
}
function asserted<T extends Fruit[]>(): T {
  return [
    1
  ] as T;
}
function safe<T extends Fruit[]>(): T {
  function nested(): number[] { return [1]; }
  return [Fruit.Apple] as T;
}
function unknown<T extends unknown[]>(): T {
  return [1] as T;
}
