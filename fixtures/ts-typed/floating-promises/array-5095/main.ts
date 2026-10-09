
function _<T, S extends Array<T | Array<T | Promise<T>>>>(
  maybePromiseArrayArray: S | undefined,
): void {
  maybePromiseArrayArray?.[0];
}
      