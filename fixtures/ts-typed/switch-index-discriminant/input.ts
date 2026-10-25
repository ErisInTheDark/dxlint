interface Array<T> { length: number; [index: number]: T }
function direct(x: string[]) {
  switch (x[0]) {
    case "hi": break;
  }
}
function alias(x: string[]) {
  const value = x[0];
  switch (value) {
    case "hi": break;
  }
}
function covered(x: string[]) {
  switch (x[0]) {
    case "hi": break;
    case undefined: break;
  }
}
function fixed(x: readonly [string]) {
  switch (x[0]) {
    case "hi": break;
  }
}
