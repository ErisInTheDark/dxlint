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
function functionAssertion() {
  (function () {
    Fn
    Array()
  }) as Fn
  Array()
}
function objectAssertion() {
  ({
    foo() {
      Object
      Array()
    }
  }) as Object
  Array()
}
function compoundAssertion() {
  previous as Fn | Object
  Array(1, 2)
}
function commentedAssertion() {
  previous as Fn /* keep assertion */
  // keep boundary
  new Array()
}
function wrappedAssertion() {
  ((previous) as Fn)
  Array()
}
