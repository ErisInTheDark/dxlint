interface Promise<T> { then(callback: (value: T) => unknown): unknown }
declare function promise(): Promise<number>;
declare const choice: boolean;
declare const uncertain: any;
const arrow = async () => promise();
async function branches() { return choice ? promise() : promise(); }
const mixed = async () => choice ? await 1 : promise();
const grouped = async () => (promise());
const awaited = async () => await promise();
const unknown = async () => uncertain;
async function nested() {
  const child = async () => promise();
  return await 2;
}
async function handling() { try { return promise(); } catch {} }
async function resource() { using item = { [Symbol.dispose]() {} }; return promise(); }
