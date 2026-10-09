type Nested = Record<string, Record<number, boolean>>;
type Recursive = { [key: string]: Record<string, Recursive> };
interface RecursiveInterface { [key: string]: Record<string, RecursiveInterface>; }
