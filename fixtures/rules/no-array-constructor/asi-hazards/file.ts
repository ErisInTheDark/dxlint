function identifiers() {
  previous
  Array()
}
function calls() {
  previous()
  Array(1, 2)
}
function assertions() {
  (() => {}) as Callable
  Array()
}
function literals() {
  42
  new Array()
}
