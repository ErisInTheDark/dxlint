declare const promise: Promise<number>;
declare const promises: Promise<number>[];
void promise;
void promises;
const callback = () => void promise;
const handled = () => void promise.catch(() => {});
const returned = () => promise;
const nested = () => !(void promise);
