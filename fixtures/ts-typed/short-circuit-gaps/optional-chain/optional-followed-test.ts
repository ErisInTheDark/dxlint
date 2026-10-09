declare const x: { ready: boolean } | undefined; declare const z: number; export const r = x && x.ready && z !== 1;
