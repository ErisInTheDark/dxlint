interface Α { x: number; } interface B{ y: string; }
interface C<T extends { tag: string } = { tag: string }> extends Base<T>, Other<string> { value: T; };
export interface D<T> { value: T; }
export declare interface E { foo: string; }
namespace JSX { interface Array<T> { foo(value: T): T[]; } }
global { interface Array<T> { foo(value: T): T[]; } }
