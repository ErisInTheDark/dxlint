type Plain = Record<string, number>;
type Numeric<T> = Record<number, T>;
type Symbolic = Box<Record<symbol, library.Value>>;
function input(value: Record<string, unknown>): Record<number, boolean> { return {}; }
type Union = Record<string | number, unknown>;
type Finite = Record<Exclude<'a' | 'b', 'a'>, unknown>;
namespace Local { type Record<K, V> = V; type Custom = Record<string, number>; }
