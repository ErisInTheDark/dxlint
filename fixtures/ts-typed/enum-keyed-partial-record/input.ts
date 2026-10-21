enum One { A }
enum Many { A, B, C }
type Optional<T> = { [P in keyof T]?: T[P] };
type Keyed<K extends keyof any, T> = { [P in K]: T };

declare const one: Optional<Keyed<One, boolean>>;
declare const many: Optional<Keyed<Many, boolean>>;
declare const direct: { [P in Many]?: boolean };
declare const oneKey: One;
declare const manyKey: Many;
declare const flag: boolean;
declare const required: Keyed<Many, boolean>;

one[oneKey] === true;
many[manyKey] === true;
direct[manyKey] === true;
flag === true;
required[Many.B] === true;
