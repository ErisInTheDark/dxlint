export enum E { Zero, One }
export class Box<T> { value!: T; }
export namespace Nested {
  export interface Value { value: string; }
  export declare const value: string;
}
