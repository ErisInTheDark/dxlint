interface Promise<T> { then(callback: (value: T) => unknown): unknown }
declare function promise(): Promise<number>;
declare const uncertain: any;
async function handling() {
  try { return await promise(); } catch { return await promise(); }
}
const object = async () => await { then(callback: () => void) {} };
const unknown = async () => await uncertain;
const untouched = async () => promise();
