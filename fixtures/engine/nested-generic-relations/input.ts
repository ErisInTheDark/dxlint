type Box<T> = { next: T };
declare function double(value: Box<Box<number>>): "number";
declare function double(value: Box<Box<string>>): "string";
declare function triple(value: Box<Box<Box<number>>>): "number";
declare function triple(value: Box<Box<Box<string>>>): "string";
declare const doubleString: Box<Box<string>>;
declare const tripleString: Box<Box<Box<string>>>;
const doubleResult = double(doubleString);
const tripleResult = triple(tripleString);
