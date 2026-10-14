interface Array<T> { length: number; [index: number]: T }
interface ReadonlyArray<T> { readonly length: number; readonly [index: number]: T }
function check(
  xs: string[],
  record: { [key: string]: number },
  known: { fixed: number; [key: string]: number },
  fixed: readonly [string, number],
  optional: readonly [string, number?],
  variadic: readonly [string, ...number[]],
  i: number,
  key: string,
) {
  const arrayRead = xs[0];
  const recordRead = record[key];
  const namedRead = known["fixed"];
  const fixedRead = fixed[0];
  const optionalRead = optional[1];
  const dynamicRead = fixed[i];
  const variadicRead = variadic[5];
  const dynamicRecordRead = known[key];
  const unionRead = fixed[0 as 0 | 1];
}
function foo(x: string[], y: string | undefined) {
  const a = x[0];
  if (typeof a === "string") {
    return;
  }
  switch (y) {
    case "hi": break;
    case a: break;
  }
}
