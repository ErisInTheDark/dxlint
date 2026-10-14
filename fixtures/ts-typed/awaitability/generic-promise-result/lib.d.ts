interface PromiseLike<T> {
  then<TResult>(onfulfilled: (value: T) => TResult): PromiseLike<TResult>;
}
export interface Promise<T> {
  then<TResult>(onfulfilled: (value: T) => TResult): Promise<TResult>;
}
type Awaited<T> = T extends null | undefined ? T :
  T extends object & { then(onfulfilled: infer F, ...args: any[]): any } ?
    F extends (value: infer V, ...args: any[]) => any ? Awaited<V> : never : T;
interface PromiseConstructor {
  resolve(): Promise<void>;
  resolve<T>(value: T): Promise<Awaited<T>>;
  resolve<T>(value: T | PromiseLike<T>): Promise<Awaited<T>>;
}
export declare var Promise: PromiseConstructor;
