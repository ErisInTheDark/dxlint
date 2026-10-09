type Plain = Record<string, number>;
type Frozen = Readonly<Record<string, library.Value>>;
type Generic = Box<Record<number, string>>;
function input(value: Record<symbol, unknown>): Record<string, boolean> { return {}; }
