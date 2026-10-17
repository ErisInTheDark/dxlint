export type IslandId = `${number},${number}`;
export namespace IslandId {
  export const marker = 1;
}
export class Box {
  value: number;
}
export namespace Box {
  export type Nested = boolean;
  export const marker = 2;
}
export interface Shape {
  value: number;
}
export namespace Shape {
  export const marker = 3;
}
export enum Direction {
  North,
  South,
}
export namespace Direction {
  export const marker = 4;
}
export namespace Types {
  export type Entry = string;
}
