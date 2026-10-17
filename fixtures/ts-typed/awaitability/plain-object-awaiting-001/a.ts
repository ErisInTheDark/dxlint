interface Promise<T> { then(callback: (value: T) => unknown): unknown; }
interface Thenable<T> { then(callback: (value: T) => unknown): unknown; }
const bare: object = {} as object;
const promised: object = {} as Promise<object>;
const mixed: any = {} as Promise<any> | Promise<object>;
const nested: any = {} as Promise<Promise<any>>;
const unknownValue: unknown = {} as unknown;
const anyValue: any = {} as any;
const structural: { value: number } = {} as { value: number };
const thenable: number = {} as Thenable<number>;
const intersection: number = {} as object & Thenable<number>;
const notThenable: { then: string } = {} as { then: string };
export {};
