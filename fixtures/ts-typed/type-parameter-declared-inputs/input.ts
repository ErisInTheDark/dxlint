declare function untyped<T>(T): T;
interface Storage {
  setItem<T>({ key: string, value: T }): Promise<void>;
}
type Guard = <T>(value: unknown) => value is T;
type Assert = <T>(value: unknown) => asserts value is T;
declare function unknownInput<T>(value: unknown): T;
declare function anyInput<T>(value: any): T;
declare function receiver<T>(this: T, value: unknown): void;
declare function related<T>(left: T, right: T): unknown;
type Hidden<T> = Missing<T>;
declare function opaque<T>(value: Hidden<T>): void;
const identity: <T>(value: T) => T = <T>(value) => value;
declare function wrapped<T>(first: unknown, second: Wrapper<T>): void;
declare function pairInput<T>(first: unknown, second): T;
export {};
