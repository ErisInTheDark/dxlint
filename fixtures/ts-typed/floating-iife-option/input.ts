declare const promise: Promise<number>;
declare const promises: Promise<number>[];
(() => promise)();
(function () { return promise; })();
(() => promises)();
promise;
true && (() => promise)();
(() => { promise; return promise; })();
