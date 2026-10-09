
function f<T extends Array<Promise<number>>>(a: T | undefined): void {
  a;
}
      