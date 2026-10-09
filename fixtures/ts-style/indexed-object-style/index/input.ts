type Plain = { [key: string]: number };
type Frozen = { readonly [key: string]: library.Value };
type Generic = Box<{ [key: number]: string }>;
function input(value: { [key: symbol]: unknown }): { [key: string]: boolean } { return {}; }
