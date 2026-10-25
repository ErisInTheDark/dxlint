type Record<K extends number, V> = { [P in K]: V };
enum Bus { A, B }
const buses: { [Bus.A](): number; [Bus.B](): string } = { [Bus.A]: () => 1, [Bus.B]: () => "" };
type Buses = typeof buses;
class Manager<B extends number, M extends Record<B, any>> {
constructor(public readonly buses: M) {}
}
class Plain<B, M> {
constructor(public readonly buses: M) {}
}
const constrained = new Manager<Bus, Buses>(buses);
const plain = new Plain<Bus, Buses>(buses);
export {};
