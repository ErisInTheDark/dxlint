// Calling `Function` answers `any`, which already includes `undefined`, so the
// fix does not widen the chain's value.
function foo(globalThis?: { Array: Function }) {
  globalThis?.Array();
}
