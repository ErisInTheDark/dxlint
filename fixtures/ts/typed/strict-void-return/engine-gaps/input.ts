type UtilityReturn<T extends (...args: any[]) => any> = T extends (...args: any[]) => infer R ? R : any;
declare function utility(cb: () => void): void;
utility((): UtilityReturn<typeof utility> => { return; });
declare const ConstructorOne: { new (cb: () => void): void };
new ConstructorOne(async () => 123);
declare const ConstructorTwo: { new (cb: () => void): void; (cb: () => unknown): void };
new ConstructorTwo(() => false);
declare function overload(cb: (arg: string) => () => void): void;
declare function overload(cb: (arg: number) => () => boolean): void;
overload((arg: number) => { return booleanCallback; });
function booleanCallback() { return true; }
class Base { cb() {} }
const key = "cb" as const;
class Derived extends Base { [key]() { return "nara"; } }
declare let factory: { f(): () => void };
factory = { f() { return () => booleanCallback; } };
class CallableGetter extends Base { get cb() { return () => 1; } }
const nullableArray: { cbs: (() => void)[] | null } = { cbs: [async () => 1, function* () { yield 1; }] };
