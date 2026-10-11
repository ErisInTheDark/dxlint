enum Fruit { Apple }
interface Uncastable { fruit: Fruit; }
interface Castable { asContainer: (this & Uncastable) | undefined; }
interface Container extends Uncastable, Castable {}
interface First extends Container { asContainer: (this & Container) | undefined; first: number; }
interface Second extends Container { asContainer: (this & Container) | undefined; second: number; }
interface Third extends Container { asContainer: (this & Container) | undefined; third: number; }
declare const container: (First & Container) | (Second & Container) | (Third & Container);
function move(): First | Second | Container | undefined {
  return container;
}
function wrong(): Fruit {
  return 1;
}
