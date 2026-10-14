function missing(value: "a" | "b") {
  switch (value) { case "a": break; }
}
function complete(value: "a" | "b") {
  switch (value) { case "a": break; case "b": break; }
}
function broad(value: string) {
  switch (value) { case "a": break; }
}
function booleanGap(value: boolean) {
  switch (value) { case true: break; }
}
function nullableGap(value: "a" | null | undefined) {
  switch (value) { case "a": break; case undefined: break; }
}
function defaultDoesNotCover(value: "a" | "b") {
  switch (value) { case "a": break; default: break; }
}
function returnNarrowed(value: "a" | "b") {
  if (value === "a") return;
  switch (value) { case "b": break; }
}
enum Choice { A, B }
function enumComplete(value: Choice) {
  switch (value) { case Choice.A: break; case Choice.B: break; }
}
function enumGap(value: Choice) {
  switch (value) { case Choice.A: break; }
}
declare const firstSymbol: unique symbol;
declare const secondSymbol: unique symbol;
function symbolGap(value: typeof firstSymbol | typeof secondSymbol) {
  switch (value) { case firstSymbol: break; }
}
const inferredFirst = Symbol("first");
const inferredSecond = Symbol.for("second");
function inferredSymbolGap(value: typeof inferredFirst | typeof inferredSecond) {
  switch (value) { case inferredFirst: break; }
}
