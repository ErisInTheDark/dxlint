/* global unusedGlobal */
export function check(unusedArg) {
  let unusedLocal;
  unusedLocal = 1;
  try {} catch (unusedError) {}
  let _used = 1;
  _used = 2;
  consume(_used);
}
