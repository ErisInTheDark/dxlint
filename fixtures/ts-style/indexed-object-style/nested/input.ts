type Nested = { [key: string]: { [key: number]: boolean } };
type Recursive = { [key: string]: { [key: string]: Recursive } };
interface RecursiveInterface { [key: string]: { [key: string]: RecursiveInterface }; }
