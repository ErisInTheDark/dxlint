type Lit = { a: number; b: string };
type FrozenLit = { readonly a: number; readonly b: string };
interface Box<T> { value: T }

declare const optional: { [K in keyof Lit]?: Lit[K] };
optional;
declare const frozen: { readonly [K in keyof Lit]: Lit[K] };
frozen;
declare const mutable: { -readonly [K in keyof FrozenLit]: FrozenLit[K] };
mutable;
declare const keys: { [K in keyof Lit]: K };
keys;
declare const lists: { [K in keyof Lit]: Lit[K][] };
lists;
declare const flags: { [K in keyof Box<number>]: boolean };
flags;
declare const two: { [K in "a" | "b"]: number };
two;
declare const record: { [P in "a" | "b"]: number };
record;
declare const getters: { [K in keyof Lit as `get${string & K}`]: Lit[K] };
getters;
