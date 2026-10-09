
function f<T extends Array<Promise<number>>>(a: T): void {
  a;
}
      