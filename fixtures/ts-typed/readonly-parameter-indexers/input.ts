interface Array<T> { length: number; [index: number]: T }
interface ReadonlyArray<T> { readonly length: number; readonly [index: number]: T }
function mutableTextIndexer(arg: { [key: string]: string }) {}
function mutableOrdinalIndexer(arg: { [key: number]: string }) {}
function readonlyTextIndexer(arg: { readonly [key: string]: string }) {}
function readonlyOrdinalIndexer(arg: { readonly [key: number]: string }) {}
interface ArrayExtender extends Array<string> { readonly property: boolean }
interface MutableProperty extends Array<string> { property: boolean }
function extendsArray(arg: ArrayExtender) {}
function extendsMutable(arg: MutableProperty) {}
interface IndexedInterface { [key: string]: string }
function interfaceIndexer(arg: IndexedInterface) {}
