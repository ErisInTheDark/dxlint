declare const n: string | undefined;
const logical = n || "fallback";
let assigned: string | undefined;
assigned ||= "fallback";
const ternary = n !== undefined ? n : "fallback";
const voidCheck = n !== void 0 ? n : "fallback";
let value: string | null = null;
if (value == null) { value = "fallback"; }
if (value == null) { value += "suffix"; }
function shadow(undefined: string, x: string | undefined) {
  return x !== undefined ? x : "fallback";
}
declare const box: { value: string | null } | undefined;
const optional = box?.value != null ? box?.value : "fallback";
declare const obj: { [key: number]: string | undefined };
declare const index: number;
const computed = obj[index + 1] !== undefined ? obj[index + 1] : "fallback";
declare let lazy: { a: string } | null;
declare function makeFoo(): { a: string };
if (lazy == null) {
  lazy ??= makeFoo();
}
if (lazy == null) {
  lazy ||= makeFoo();
}
if (lazy == null) lazy ??= makeFoo();
if (lazy == null) lazy ||= makeFoo();
