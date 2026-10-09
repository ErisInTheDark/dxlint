interface Thenable { then(ok: (value: number) => void, fail: () => void): void }
declare const values: Thenable[];
values;
namespace Local { export class Promise<T> { then(ok: (value: T) => void, fail: () => void): void {} } declare const local: Promise<number>[]; local; }
