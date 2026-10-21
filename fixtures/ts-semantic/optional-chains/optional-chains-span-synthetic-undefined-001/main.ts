// The `undefined` an optional call adds is gone once the chain guards its
// receiver, so the guard on the third operand still extends the chain.
declare const foo: (() => { bar: number } | null) | null;
foo === null || foo?.() === null || foo?.().bar === null;
