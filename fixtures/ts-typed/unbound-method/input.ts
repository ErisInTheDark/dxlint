import { Base, console as foreignConsole } from "./methods";

type VoidAlias = void;
class Methods extends Base<Methods> {
  ordinary(): void {}
  safe(this: void): void {}
  alias(this: VoidAlias): void {}
  wrapped(this: (void)): void {}
  functionField: () => void = function() {};
  safeField: () => void = function(this: void) {};
  arrowField = () => {};
  wrappedField = (function() {});
  typedField: () => void;
  static staticMethod(): void {}
}
declare const instance: Methods;
const ordinary = instance.ordinary;
const receiver = instance.alias;
const wrappedReceiver = instance.wrapped;
const inherited = instance.inherited;
const functionField = instance.functionField;
const staticMethod = Methods.staticMethod;
const safe = instance.safe;
const safeField = instance.safeField;
const arrowField = instance.arrowField;
const wrappedField = instance.wrappedField;
const typedField = instance.typedField;
instance.ordinary();
instance.ordinary.call(instance);
if (instance.ordinary) {}
const exists = typeof instance.ordinary;
const equal = instance.ordinary === instance.safe;
const fallback = instance.ordinary && instance.safe;
const unsafeFallback = true && instance.ordinary;
const unsafeArgument = consume(instance.ordinary);
declare function consume(value: unknown): void;
const { ordinary: destructured } = instance;
function parameter({ ordinary }: Methods): void {}
function nested({ child: { ordinary } }: { child: Methods }): void {}
function defaulted({ ordinary } = instance): void {}
declare const union: Methods | { ordinary: () => void };
const fromUnion = union.ordinary;
const literal = instance["ordinary"];
declare const key: string;
const dynamic = instance[key];
const fromForeignConsole = foreignConsole.log;
const fromConsole = console.log;
const { log } = console;
const collator = new Intl.Collator();
const compare = collator.compare;
class Collator { compare(a: string, b: string): number { return 0; } }
const localCompare = new Collator().compare;
const object = {
  method() {},
  functionField: function() {},
  safeField: function(this: void) {},
  arrowField: () => {},
};
const objectMethod = object.method;
const objectFunction = object.functionField;
const objectSafe = object.safeField;
const objectArrow = object.arrowField;
declare const alias: "safe";
const { [alias]: computedPattern } = instance;
class ArrayDerived extends Array {}
const safeInheritedBuiltin = ArrayDerived.isArray;
