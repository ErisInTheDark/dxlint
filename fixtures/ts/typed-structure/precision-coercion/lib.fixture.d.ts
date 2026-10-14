interface Object { toString(): string; toLocaleString(): string; valueOf(): Object; }
type Mapped = { [K in 'toString']: () => string };
declare const mapped: Mapped;
'' + mapped;
mapped.toString();
interface UnknownChild extends Missing {}
declare const unresolved: UnknownChild;
'' + unresolved;
unresolved.toString();
interface Plain extends Object {}
declare const plain: Plain;
plain.toString();
