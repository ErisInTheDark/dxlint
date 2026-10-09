declare const x: { f?(): boolean }; export const result = x.f?.() === false;
