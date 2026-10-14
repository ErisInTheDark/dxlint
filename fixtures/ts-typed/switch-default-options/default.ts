function missingDefault(value: string) {
  switch (value) { case "a": break; }
}
function commentDefault(value: string) {
  switch (value) {
    case "a": break;
    // no default
  }
}
function actualDefault(value: string) {
  switch (value) { default: break; }
}
