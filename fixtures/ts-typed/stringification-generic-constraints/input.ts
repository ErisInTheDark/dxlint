type Dictionary = { [key: string]: string };
function constrained<T extends string | Dictionary>(value: T) {
  String(value);
}
function unknown<T>(value: T) {
  String(value);
}
function primitive<T extends string>(value: T) {
  String(value);
}
function custom<T extends { toString(): string }>(value: T) {
  String(value);
}
function object<T extends Dictionary>(value: T) {
  String(value);
}
