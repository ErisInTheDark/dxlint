function bounded<T extends void>(value: T) {
  value ?? 1;
}
function undefinedValue(value: undefined) {
  value ?? 1;
}
function wide(value: void | string) {
  value ?? 1;
}
function exact(value: void) {
  value ?? 1;
}
function unresolved<T>(value: T) {
  value ?? 1;
}
type VoidAlias = void;
function aliased<T extends VoidAlias>(value: T) {
  value ?? 1;
}
function absent<T extends undefined>(value: T) {
  value ?? 1;
}
declare const stable: number;
stable ?? 1;
