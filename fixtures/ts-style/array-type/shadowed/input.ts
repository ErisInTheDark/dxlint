type Array<T> = T;
type ReadonlyArray<T> = T;
type Readonly<T> = T;
type Local = Array<number>;
type ReadonlyLocal = ReadonlyArray<number>;
type WrapperLocal = Readonly<string[]>;
