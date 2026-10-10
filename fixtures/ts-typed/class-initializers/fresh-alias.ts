const ready = true;
class C { options = { ready }; }
declare const x: C | undefined;
declare const unrelated: boolean;
export const result = x && x.options.ready && unrelated;
