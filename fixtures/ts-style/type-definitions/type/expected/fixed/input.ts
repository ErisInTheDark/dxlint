type Α = { x: number; } type B = { y: string; }
type C<T extends { tag: string } = { tag: string }> = { value: T; } & Base<T> & Other<string>;
export type D<T> = { value: T; }
export declare type E = { foo: string; }
namespace JSX { type Array<T> = { foo(value: T): T[]; } }
global { type Array<T> = { foo(value: T): T[]; } }
