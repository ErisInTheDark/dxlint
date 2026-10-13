function generic<T extends string>(value: T) { const parameter = value; }
declare const brand: string & { readonly tag: unique symbol };
const branded = brand;
declare const text: string;
const scalar = text;
declare const dynamic: any & string;
const absorbed = dynamic;
declare const opaque: unknown;
if (typeof opaque === "string") { const narrowed = opaque; }
declare function identity<T>(value: T): T;
const concrete = identity("text");

interface Merged<T> { first: T }
interface Merged<T> { second: T }
declare const mergedDynamic: Merged<any>;
declare const mergedText: Merged<string>;
const mergedUnsafe: Merged<string> = mergedDynamic;
const mergedSafe: Merged<string> = mergedText;
const dynamicSafe: unknown = absorbed;
