interface Array<T> { length: number; [index: number]: T }
interface ReadonlyArray<T> { readonly length: number; readonly [index: number]: T }
function primitive(arg: unknown, scalar: string) {}
function mutableArray(arg: string[]) {}
function readonlyArray(arg: readonly string[]) {}
function methodValue(arg: { method(): void }) {}
function readonlyMethod(arg: { readonly method: () => void }) {}
class Properties {
  constructor(private hidden: string[], readonly frozen: readonly string[]) {}
}
function defaultMutable(arg = []) {}
function defaultReadonly(arg = [] as readonly string[]) {}
function getterScalar(arg: { get item(): string }) {}
function getterMutable(arg: { get item(): string[] }) {}
function constrainedParameter<T extends string[]>(arg: T) {}
