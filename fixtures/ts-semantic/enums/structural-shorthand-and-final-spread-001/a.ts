enum Fruit { Apple }
declare const fruit: number;
declare const source: { fruit: number };
const shorthand: { fruit: Fruit } = {
  fruit
};
const spread: { fruit: Fruit } = {
  ...source
};
const overridden: { fruit: Fruit } = { ...source, fruit: Fruit.Apple };
declare const opaque: unknown;
const unknownSpread: { fruit: Fruit } = { ...opaque };
function safe(): void {
  const fruit: Fruit = Fruit.Apple;
  const shorthand: { fruit: Fruit } = { fruit };
  const spread: { fruit: Fruit } = { ...shorthand };
}
