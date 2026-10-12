declare const promise: Promise<number>;
declare const promises: Promise<number>[];
async function check() {
  await promise;
  await promises;
  void promises;
}
