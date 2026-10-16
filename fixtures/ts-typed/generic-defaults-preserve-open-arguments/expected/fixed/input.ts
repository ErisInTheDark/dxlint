import { Dropdown, type Arguments, type Description } from "./base";
class Forward<T extends string = never> extends Dropdown<T> {}
function alias<T extends string = never>() {
  type Hidden = T;
  return new Dropdown<Hidden>();
}
class Action<A extends unknown[], AV extends unknown[] = Arguments<A>> implements Description<A, AV> { value!: AV; }
new Dropdown();
