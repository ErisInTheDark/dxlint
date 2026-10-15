interface Child<A, B> extends Set<B> {}
declare const promisedChild: Child<number, Promise<number>>;
declare const plainChild: Child<Promise<number>, number>;
declare const optionalTuple: [Promise<number>?];
declare const repeatedTuple: [...number[]];
declare const nested: number[][];
declare const promised: Promise<number>[];
Promise.all(promisedChild);
Promise.all(plainChild);
Promise.all(optionalTuple);
Promise.all(repeatedTuple);
Promise.all(nested);
Promise.all(promised);
