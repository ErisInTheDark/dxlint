type Extract<T, U> = T extends U ? T : never;
type Tree = Leaf | Branch<Tree>;
interface Leaf { type: "leaf"; }
interface Branch<T> { type: "branch"; child: T; }
const isTreeOfType = <Kind extends Tree["type"]>(kind: Kind) =>
  (node: Tree | null): node is Extract<Tree, { type: Kind }> => node?.type === kind;
