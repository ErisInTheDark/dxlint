declare const x: { ready: boolean }; declare const z: boolean; export const r = z || (x && x.ready && !x.ready);
