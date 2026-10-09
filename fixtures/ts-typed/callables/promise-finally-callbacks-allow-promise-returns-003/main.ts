
        interface Thenable { then(callback: (value: number) => unknown): unknown; finally(callback: () => void): void }
        declare const custom: Thenable;
        custom.finally(async () => {});
        const method = "finally";
        custom[method](async () => {});
    