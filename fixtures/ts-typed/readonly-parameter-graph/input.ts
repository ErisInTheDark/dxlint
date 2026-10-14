interface Array<T> { length: number; [index: number]: T }
interface ReadonlyArray<T> { readonly length: number; readonly [index: number]: T }
function unallowedChild(arg: { readonly child: { mutable: string[] } }) {}

interface Phantom<T> {}
function phantom(arg: Phantom<string[]>) {}
function readonlyElements(arg: readonly (readonly string[])[]) {}
function mutableElements(arg: readonly string[][]) {}

type Branded = string & { mutableBrand: unique symbol };
function branded(arg: Branded) {}

interface FrozenSelf { readonly self: FrozenSelf }
interface MutableSelf { readonly self: MutableSelf; mutable: MutableSelf }
interface FrozenA { readonly next: FrozenB }
interface FrozenB { readonly next: FrozenA }
interface MutableA { readonly next: MutableB }
interface MutableB { next: MutableA }
function frozenCycle(arg: FrozenSelf) {}
function mutableCycle(arg: MutableSelf) {}
function frozenPair(arg: FrozenA) {}
function mutablePair(arg: MutableA) {}

function callback(arg: () => void) {}
function mutableCallable(arg: (() => void) & { property: number }) {}
class Constructor {}
function constructorValue(arg: typeof Constructor) {}

type Merged = { readonly same: string } & { same: string };
type Separate = { readonly same: string } & { other: string };
function mergedReadonly(arg: Merged) {}
function separateMutable(arg: Separate) {}

class PrivateScalar { #value = 1; method() {} }
class PrivateMutable { #value: string[] = []; method() {} }
function privateScalar(arg: PrivateScalar) {}
function privateMutable(arg: PrivateMutable) {}
function nestedBrand(arg: { readonly brand: Branded }) {}
function mixedBrand(arg: Branded | string) {}
function readonlyTuple(arg: readonly [string]) {}
function nestedTuple(arg: readonly [string[]]) {}
function mutableTuple(arg: [string]) {}
interface InterfaceTag { mutableBrand: number }
function interfaceBranded(arg: string & InterfaceTag) {}
function readonlySpread(arg: readonly [...string[]]) {}
function mutableSpread(arg: readonly [...string[][]]) {}
interface GenericFrozen<T> { readonly next: GenericFrozen<T>; readonly item: T }
function genericFrozen(arg: GenericFrozen<string>) {}
function genericMutable(arg: GenericFrozen<string[]>) {}
