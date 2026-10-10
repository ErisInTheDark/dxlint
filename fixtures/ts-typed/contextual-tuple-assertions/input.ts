declare function map<U>(callback: (value: number) => U): U[];
map(x => [x, "A" + x] as [number, string]);
const mapped: [number, string][] = map(x => [x, "A" + x] as [number, string]);
type LiteralTuple = [3, "hi", "bye"];
const assertedTuple = [3, "hi", "bye"] as LiteralTuple;
const angleTuple = <LiteralTuple>[3, "hi", "bye"];
