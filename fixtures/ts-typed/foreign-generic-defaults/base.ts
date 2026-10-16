export class Box<T = string> { value?: T; }
export interface Shape<T = string> { value: T; }
export type Alias<T = string> = T;
export declare function make<T = string>(): T;
export declare function overloaded<T = string>(value: number): T;
export declare function overloaded<T = number>(value: string): T;
