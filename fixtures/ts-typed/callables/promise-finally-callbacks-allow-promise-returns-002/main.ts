
        declare const promise: Promise<void>;
        promise.finally(async () => {});
        const method = "finally";
        promise[method](async () => {});
        (promise.finally)(async () => {});
        interface Derived extends Promise<void> {}
        declare const derived: Derived;
        derived.finally(async () => {});
    