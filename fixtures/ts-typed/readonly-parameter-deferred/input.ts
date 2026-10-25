interface Array<T> { length: number; [index: number]: T }
interface ReadonlyArray<T> { readonly length: number; readonly [index: number]: T }
type Readonly<T> = { readonly [K in keyof T]: T[K] };
type AllowedAlias = { mutable: string[] };
interface AllowedNested { mutable: string[] }
function allowedRoot(arg: AllowedAlias) {}
function allowedChild(arg: { readonly child: AllowedNested }) {}
function unallowedChild(arg: { readonly child: { mutable: string[] } }) {}
function mappedReadonly(arg: Readonly<{ scalar: string }>) {}
function mappedShallow(arg: Readonly<{ nested: { scalar: string } }>) {}
interface LocalSymbol { mutable: string[] }
function wrongSource(arg: LocalSymbol) {}
type FrozenChoice<T> = T extends string ? readonly string[] : readonly number[];
type MutableChoice<T> = T extends string ? readonly string[] : number[];
function frozenConditional<T>(arg: FrozenChoice<T>) {}
function mutableConditional<T>(arg: MutableChoice<T>) {}
function genericIntersection<T>(arg: T & string[]) {}
