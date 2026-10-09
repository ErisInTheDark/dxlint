type Nested = Array<Array<string>>;
type ReadonlyNested = ReadonlyArray<object>[];
type Mixed = Array<ReadonlyArray<object>>[];
type Generic = Box<Array<Box<string>>[]>;
