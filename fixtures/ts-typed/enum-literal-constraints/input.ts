enum Fruit {
  Apple,
}

function takesFruit<T extends Fruit>(fruit: T): void {}
class Box<T extends Fruit> {
  constructor(public fruit: T) {}
}

takesFruit(1);
new Box(1);
takesFruit(Fruit.Apple);
takesFruit(0);
