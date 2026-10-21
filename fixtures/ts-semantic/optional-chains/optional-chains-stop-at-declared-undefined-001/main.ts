// `undefined` declared by the call itself is not the synthetic one an optional
// link adds, so `!== null` does not clear it and the chain stops.
declare const foo: { f: () => { baz: number } | null | undefined } | null;
foo !== null && foo.f?.() !== null && foo.f?.().baz;
