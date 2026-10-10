type Α = { x: number; }; type B={ y: string; };
type C = ((({ value: boolean })));
export type D<T extends { tag: string } = { tag: string }> = { value: T; };
export declare type E = { foo: string; };
type F = { value: string }
type Other = string;
type Mapped<T> = { [K in keyof T]: T[K] };
type Union = { a: number } | { b: number };
