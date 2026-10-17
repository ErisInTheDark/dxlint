interface Promise<T> { then(fulfilled: (value: T) => unknown): unknown; }
interface PromiseConstructor { reject<T = never>(reason?: any): Promise<T>; }
declare const Promise: PromiseConstructor;
declare const uncertain: unknown;
async function opaque() {
  return Promise.reject();
}
async function unknownValue() {
  return uncertain;
}
async function ordinary() {
  return 1;
}
export {};
