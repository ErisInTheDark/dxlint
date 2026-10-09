
            declare const x: string | null | undefined;
            declare function f(): string | undefined;
            x !== null && x !== undefined ? x : "fallback";
            x !== undefined ? x : "fallback";
            f() ? f() : "fallback";
        