export {};
declare const raw: Function;
raw();
new raw();
raw``;
interface VoidCallable extends Function { (): void }
declare const onlyVoid: VoidCallable;
onlyVoid();
new onlyVoid();
onlyVoid``;
interface ValueCallable extends Function { (): number }
declare const valueResult: ValueCallable;
valueResult();
new valueResult();
valueResult``;
interface ConstructorOnly extends Function { new (): void }
declare const constructOnly: ConstructorOnly;
constructOnly();
new constructOnly();
constructOnly``;
interface InheritedVoid extends VoidCallable { own: number }
declare const inheritedVoid: InheritedVoid;
inheritedVoid();
new inheritedVoid();
inheritedVoid``;
interface InheritedConstructor extends ConstructorOnly { own: number }
declare const inheritedConstruct: InheritedConstructor;
inheritedConstruct();
new inheritedConstruct();
inheritedConstruct``;
declare const intersectionVoid: Function & (() => void);
intersectionVoid();
new intersectionVoid();
intersectionVoid``;
declare const intersectionValue: Function & (() => number);
intersectionValue();
new intersectionValue();
intersectionValue``;
function returnsVoid(): void {}
declare const ordinaryVoid: typeof returnsVoid & Function;
ordinaryVoid();
new ordinaryVoid();
ordinaryVoid``;
function returnsValue(): number { return 1 }
declare const ordinaryValue: typeof returnsValue & Function;
ordinaryValue();
new ordinaryValue();
ordinaryValue``;
