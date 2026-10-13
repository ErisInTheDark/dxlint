interface Array<T> {}
interface Promise<T> { then(callback: (value: T) => unknown): unknown }
type Awaited<T> = T extends Promise<infer U> ? Awaited<U> : T;
interface IteratorResult<Y, R> { value: Y | R }
interface Generator<Y, R, N> { next(...input: N[]): IteratorResult<Y, R> }
interface AsyncGenerator<Y, R, N> { next(...input: N[]): Promise<IteratorResult<Y, R>> }
