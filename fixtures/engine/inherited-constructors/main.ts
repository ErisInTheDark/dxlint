import { ImportedBase } from "./base";
class Parent {
  parent = 1;
  constructor(value: number) {}
}
class Child extends Parent {
  child = "child";
}
class Grandchild extends Child {
  grandchild = true;
}
class Imported extends ImportedBase {
  imported = "imported";
}
class OverloadedBase {
  constructor(value: number);
  constructor(value: string);
  constructor(value: number | string) {}
}
class Overloaded extends OverloadedBase {
  own = 1;
}
declare const missing: unknown;
class Unresolved extends missing {}
new Child(1).child;
new Child(1).parent;
new Grandchild(1).grandchild;
new Grandchild(1).child;
new Grandchild(1).parent;
new Imported('input').imported;
new Imported('input').base;
new Overloaded(1).own;
new Overloaded('input').own;
const spread = { ...new Child(1) };
new Unresolved();
