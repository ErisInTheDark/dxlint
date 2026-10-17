declare const n: number;
const numeric = n;
const literal = 3;
const widened = 3 as number;
let retained = 3 as 3;
declare const object: { a: number };
object as { b: number };
type Readonly<T> = { readonly [K in keyof T]: T[K] };
interface Obj { id: number; }
declare const readonly: Readonly<Obj>;
const mutable = readonly as Obj;
