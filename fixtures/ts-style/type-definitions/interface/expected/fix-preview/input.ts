interface Α { x: number; } interface B { y: string; }
interface C { value: boolean }
export interface D<T extends { tag: string } = { tag: string }> { value: T; }
export declare interface E { foo: string; }
interface F { value: string }
type Other = string;
type Mapped<T> = { [K in keyof T]: T[K] };
type Union = { a: number } | { b: number };
