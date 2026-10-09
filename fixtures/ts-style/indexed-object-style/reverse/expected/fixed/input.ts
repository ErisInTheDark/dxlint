type Plain = { [key: string]: number };
type Numeric<T> = { [key: number]: T };
type Symbolic = Box<{ [key: symbol]: library.Value }>;
function input(value: { [key: string]: unknown }): { [key: number]: boolean } { return {}; }
type Union = Record<string | number, unknown>;
type Finite = Record<Exclude<'a' | 'b', 'a'>, unknown>;
namespace Local { type Record<K, V> = V; type Custom = Record<string, number>; }
