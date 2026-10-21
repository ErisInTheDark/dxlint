interface SymbolConstructor { readonly iterator: unique symbol }
declare const Symbol: SymbolConstructor;
interface Generator<T> { next(): { value: T; done: boolean } }

declare function getFunction(): () => void;
const fromCall = { ...getFunction() };

interface SetLike<T> { add(value: T): this }
interface SetLikeConstructor { new <T>(value: T): SetLike<T>; readonly prototype: SetLike<any> }
declare const SetLike: SetLikeConstructor;
const indirect = SetLike<number>;
const indirectSpread = { ...indirect };
const directSpread = { ...SetLike<number> };
const constructorSpread = { ...SetLike };

type IterableLike = { [Symbol.iterator]: () => Generator<string> };
type AllowedIterable = { [Symbol.iterator]: () => Generator<string> };
declare const iterable: IterableLike;
declare const allowed: AllowedIterable;
const iterableSpread = { ...iterable };
const allowedSpread = { ...allowed };

declare const annotated: () => void;
annotated.value = 1;
const annotatedSpread = { ...annotated };
const inferred = () => {};
inferred.value = 1;
const inferredSpread = { ...inferred };
