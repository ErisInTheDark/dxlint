class Box<T> {
constructor(readonly value: T) {}
read(): T { return this.value; }
}
class Plain {
constructor(readonly value: number) {}
read(): number { return this.value; }
}
const inferred = new Box(1);
const explicit = new Box<string>("x");
const plain = new Plain(1);
export {};
