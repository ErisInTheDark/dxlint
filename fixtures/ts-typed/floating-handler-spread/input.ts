declare const promise: Promise<number>;
declare const handlers: (() => void)[];
promise.then(...[], () => {});
promise.then(() => {}, ...handlers);
promise.catch(...handlers);
promise.then(() => {}, () => {}, ...handlers);
promise.catch(() => {}, ...handlers);
