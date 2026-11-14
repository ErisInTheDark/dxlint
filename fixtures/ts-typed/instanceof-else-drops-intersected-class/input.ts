interface Box { open: boolean }
interface Box { size: number }
declare var Box: { prototype: Box; new (): Box };
interface Plain { count: number }
interface Holder { item?: Plain | Box }
type Defined<T> = T & {};
declare function use(value: unknown): void;
export function intersected(s: Defined<Holder["item"]>) {
  const e = s instanceof Box ? 0 : use(s);
  return e;
}
export function union(s: Plain | Box) {
  const e = s instanceof Box ? 0 : use(s);
  return e;
}
