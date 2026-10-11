interface Promise<T> {}
interface PromiseLike<T> {}
type V = void;

function resolved(flag: boolean): V {
  if (flag) return;
  return 1;
}
function resolvedUnion(flag: boolean): V | number {
  if (flag) return;
  return 1;
}
async function resolvedPromise(flag: boolean): Promise<V> {
  if (flag) return;
  return 1;
}
async function resolvedPromiseLike(flag: boolean): PromiseLike<V> {
  if (flag) return;
  return 1;
}
function synchronousPromise(flag: boolean): Promise<V> {
  if (flag) return;
  return 1;
}
function nonvoid(flag: boolean): number {
  if (flag) return;
  return 1;
}
function unannotated(flag: boolean) {
  if (flag) return;
  return 1;
}
