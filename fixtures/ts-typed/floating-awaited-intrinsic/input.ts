declare const array: Array<Promise<number>>;
declare const tuple: [number, number];
declare const nested: Promise<Array<Promise<number>>>;
declare const promise: Promise<number>;
declare const ordinary: number[];
declare const uncertain: unknown;
declare const dynamic: any;
async function check() {
  await array;
  await tuple;
  await nested;
  await promise;
  await ordinary;
  await uncertain;
  await dynamic;
  void array;
}
