export const present = 1;
const privateName = 2;
export interface TypeOnly {}
export namespace Nested {
  export const leaf = 'ok';
  const hidden = 3;
  export enum Inner { Zero, One }
}
export enum E { Zero, One }
export namespace E { export type Text = 'merged'; }
export type Box<T> = T;
export namespace ExportDot.Inner {
  export const present = 13;
}
