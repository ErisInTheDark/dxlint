interface Promise<T> { then(callback: (value: T) => unknown): unknown }
declare function promise(): Promise<number>;
declare const choice: boolean;
declare const uncertain: any;
const arrow = async () => await promise();
async function branches() { return choice ? await promise() : await promise(); }
const mixed = async () => choice ? 1 : await promise();
const grouped = async () => (await promise());
const awaited = async () => await promise();
const unknown = async () => uncertain;
async function nested() {
  const child = async () => await promise();
  return 2;
}
async function handling() { try { return promise(); } catch {} }
async function resource() { using item = { [Symbol.dispose]() {} }; return promise(); }
