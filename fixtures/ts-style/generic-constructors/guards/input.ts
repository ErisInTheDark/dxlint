const correct = new Foo<string>();
const different: Foo<string> = new Bar();
const both: Foo<string> = new Foo<string>();
const qualified: ns.Foo<string> = new ns.Foo();
const typed: Uint8Array<ArrayBufferLike> = new Uint8Array();
const arrow = (x: Foo<string> = new Foo()) => x;
class Float32Array<T> {}
const local: Float32Array<ArrayBufferLike> = new Float32Array();
export {};
