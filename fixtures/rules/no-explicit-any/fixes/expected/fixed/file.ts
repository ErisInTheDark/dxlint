let value: unknown;
type Values = Array<unknown>;
type Keys = PropertyKey;
function collect(...args: unknown[]): unknown { return args; }
type Nested = (unknown | string)[];
