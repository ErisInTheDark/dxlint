declare const f: (() => boolean) | undefined; export const result = f?.() === false;
