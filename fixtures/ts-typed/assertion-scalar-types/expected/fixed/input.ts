declare const name_: string;
const value: number = 5;
const unknownBridge = value;
const anyBridge = value;
const angleBridge = value;
declare function accept(text: string): void;
accept(`hello ${name_}`);
let staticTemplate = `hello` as string;
const widened = 1 as number;
let literal = 1 as 1;
const different = 1 as 2;
declare const dynamic: any;
const opaque = dynamic as number;
function generic<T>(x: T) { return x as number; }
type ValuePath = 'values' | `values.${string}`;
declare function apply(paths: ValuePath[]): void;
function update(id: string) { apply([`values.${id}` as ValuePath]); }
const templateConst = `hello ${name_}` as const;
