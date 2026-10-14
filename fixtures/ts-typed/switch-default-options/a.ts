function extraDefault(value: "a") {
  switch (value) { case "a": break; default: break; }
}
function defaultCovers(value: "a" | "b") {
  switch (value) { case "a": break; default: break; }
}
function commentCovers(value: "a" | "b") {
  switch (value) {
    case "a": break;
    // intentional
  }
}
function needsDefault(value: string) {
  switch (value) { case "a": break; }
}
function broadComment(value: string) {
  switch (value) {
    case "a": break;
    // intentional
  }
}
function twoMissing(value: "a" | number) {
  switch (value) {}
}
function replacedComment(value: "a" | "b") {
  switch (value) {
    case "a": break;
    // no default
  }
}
function booleanComplete(value: boolean) {
  switch (value) { case true: break; case false: break; }
}
function unionCase(value: "a" | "b") {
  switch (value) { case ("a" as "a" | "b"): break; default: break; }
}
const inferredSymbol = Symbol("symbol");
function symbolComplete(value: typeof inferredSymbol | 2) {
  switch (value) { case inferredSymbol: break; case 2: break; }
}
