interface Promise<T> { then(callback: (value: T) => unknown): unknown }
declare function promise(): Promise<number>;
declare const uncertain: any;
declare const choice: boolean;
async function primitive() { return 1; }
const object = async () => ({ a: 1 });
const member = async () => ({ a: 1 }.a);
const grouped = async () => ({ a: 1 });
const outer = async () => ({ a: 1 });
const commented = async () => /* keep */ ({ a: 1 });
const branches = async () => choice ? { a: 1 } : 2;
async function lineComment() {
  return (// keep
    1);
}
async function blockNewline() {
  return (/* keep
  */ 1);
}
async function plainNewline() {
  return 1;
}
async function handling() {
  try { return promise(); } catch { return promise(); }
}
async function finallyHandling() {
  try { return promise(); } finally { void 0; }
}
async function resource() {
  using item = { [Symbol.dispose]() {} };
  return promise();
}
async function nestedResource() {
  using item = { [Symbol.dispose]() {} };
  const child = async () => promise();
  return 0;
}
const unknown = async () => await uncertain;
const untouched = async () => promise();
