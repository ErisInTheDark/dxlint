interface C { f?: (x: number) => boolean } declare const x: C; export const result = x.f?.(1) === false;
