interface Promise<T> { then(callback: (value: T) => unknown): unknown; }
enum Fruit { Apple }
interface FruitNode { fruit: Fruit; next: FruitNode; }
interface NumberNode { fruit: number; next: NumberNode; }
declare const numbers: NumberNode;
declare const fruits: FruitNode;
const bad: FruitNode = numbers;
const safe: FruitNode = fruits;
declare const opaque: unknown;
const unknownSource: FruitNode = opaque;
declare const promised: Promise<number>;
declare const promisedFruit: Promise<Fruit>;
async function direct(): Promise<Fruit> {
  return 1;
}
async function wrapped(): Promise<Fruit> {
  return promised;
}
async function safeWrapped(): Promise<Fruit> {
  return promisedFruit;
}
async function unknownWrapped(): Promise<Fruit> {
  return opaque;
}
