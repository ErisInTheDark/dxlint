declare const x: boolean | { ready: boolean }; export const r = x === true || (x && x.ready);
