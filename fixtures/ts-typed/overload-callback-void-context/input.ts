declare function foo(x: null, cb: () => void): void;
declare function foo(x: unknown, cb: () => any): void;
foo({}, async () => {});
export {};
