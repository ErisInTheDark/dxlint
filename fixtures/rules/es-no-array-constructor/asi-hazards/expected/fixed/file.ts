function identifiers() {
  previous
  ;[]
}
function calls() {
  previous()
  ;[1, 2]
}
function assertions() {
  (() => {}) as Callable
  []
}
function literals() {
  42
  ;[]
}
function functionAssertion() {
  (function () {
    Fn
    ;[]
  }) as Fn
  []
}
function objectAssertion() {
  ({
    foo() {
      Object
      ;[]
    }
  }) as Object
  []
}
function compoundAssertion() {
  previous as Fn | Object
  [1, 2]
}
function commentedAssertion() {
  previous as Fn /* keep assertion */
  // keep boundary
  []
}
function wrappedAssertion() {
  ((previous) as Fn)
  ;[]
}
