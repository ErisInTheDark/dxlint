enum Fruit { Apple }
function array(
  [fruit]: [Fruit] = [
    1
  ]
): void {}
function object(
  { fruit }: { fruit: Fruit } = {
    fruit: 1
  }
): void {}
function safe(
  [fruit]: [Fruit] = [Fruit.Apple],
  { other }: { other: Fruit | number } = { other: 1 },
): void {}
function unknown(
  { value }: { value: unknown } = { value: 1 },
): void {}
const { fruit = 1 }: { fruit?: Fruit } = {};
const { safeFruit = Fruit.Apple }: { safeFruit?: Fruit } = {};
