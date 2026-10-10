interface Promise<T> { then(callback: (value: T) => unknown): unknown }
declare function promise(): Promise<number>;
declare const uncertain: any;
const preserved = async () => await promise();
const primitive = async () => 1;
const unknown = async () => await uncertain;
async function handling() {
  try { return promise(); } catch { return await promise(); }
}
async function resource() {
  using item = { [Symbol.dispose]() {} };
  return promise();
}
