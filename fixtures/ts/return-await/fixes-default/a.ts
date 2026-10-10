interface Promise<T> { then(callback: (value: T) => unknown): unknown }
declare function promise(): Promise<number>;
declare const uncertain: any;
declare const choice: boolean;
async function primitive() { return await 1; }
const object = async () => await { a: 1 };
const member = async () => await { a: 1 }.a;
const grouped = async () => await ({ a: 1 });
const outer = async () => (await { a: 1 });
const commented = async () => await /* keep */ { a: 1 };
const branches = async () => choice ? await { a: 1 } : await 2;
async function lineComment() {
  return await // keep
    1;
}
async function blockNewline() {
  return await /* keep
  */ 1;
}
async function plainNewline() {
  return await
    1;
}
async function handling() {
  try { return promise(); } catch { return await promise(); }
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
  const child = async () => await promise();
  return 0;
}
const unknown = async () => await uncertain;
const untouched = async () => promise();
