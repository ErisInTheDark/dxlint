interface Array<T> { length: number; [index: number]: T }
declare const intersection: any[] & { n: number };
intersection.n;
const constrained = <T extends unknown[]>(...items: T) => items.length;
declare const conflict: { n: number } & { n: string };
conflict.n;
function loose<T>(unconstrained: T) {
  return unconstrained.missing;
}
