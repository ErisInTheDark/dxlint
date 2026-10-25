export type Id = `${number},${number}`;
export namespace Id {
  export function isValid(id: string): boolean { return id.length > 0; }
  export type Text = string;
}
let qualified: Id.Text;
type Single = `1,2`;
let a: Id;
a = `1,2`;
let b: Single;
b = `1,2`;
class Holder {
  islandId: Id;
  update() {
    this.islandId = `3,4`;
  }
}
declare const impossible: never;
export {};
