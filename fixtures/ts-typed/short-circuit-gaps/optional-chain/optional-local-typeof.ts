declare const x: { keyboard?: object } | undefined; export const r = typeof x !== 'undefined' && typeof x.keyboard !== 'undefined';
