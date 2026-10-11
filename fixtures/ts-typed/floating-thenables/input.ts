declare function custom(): PromiseLike<number>;
custom();
custom().then(() => {}, () => {});
custom().then(() => {}, 123 as any);
async function test() {
  class CatchableThenable {
    then(callback: () => void, rejection: () => void): CatchableThenable {
      return new CatchableThenable();
    }
  }
  const thenable = new CatchableThenable();
  thenable;
  thenable.then(() => {});
  [thenable];
}
declare const nameOnly: { then: number };
nameOnly;
[nameOnly];
declare const single: { then(callback: () => void): unknown };
single;
[single];
declare const badSecond: { then(callback: () => void, rejection: number): unknown };
badSecond;
[badSecond];
declare const optional: { then(callback: (() => void) | null, rejection?: () => void): unknown };
optional;
[optional];
interface Parent<T> { then(callback: T, rejection: T): unknown; }
interface Inherited extends Parent<() => void> {}
declare const inherited: Inherited;
inherited;
[inherited];
declare const overloaded: { then(value: number): unknown; then(callback: () => void, rejection: () => void): unknown };
overloaded;
[overloaded];
declare const generic: { then<T extends () => void>(callback: T, rejection: T): unknown };
generic;
[generic];
declare const mixed: Inherited | null;
mixed;
declare const augmented: Inherited & { tag: string };
augmented;
[augmented];
declare const customs: PromiseLike<number>[];
customs;
declare const nested: PromiseLike<number>[][];
nested;
declare const builtin: Promise<number>;
builtin;
[builtin];
declare const tuple: readonly [PromiseLike<number>, number];
tuple;
declare const unknown: unknown;
unknown;
declare const objectSignature: { then: { (callback: () => void, rejection: { (): void }): unknown } };
objectSignature;
[objectSignature];
declare const objectCallback: { then(callback: { (): void }, rejection: { (): void }): unknown };
objectCallback;
[objectCallback];
declare const nullableThen: { then: ((callback: () => void, rejection: () => void) => unknown) | null };
nullableThen;
[nullableThen];
interface CallbackBase { (): void; }
interface InheritedCallback extends CallbackBase {}
declare const inheritedCallback: { then(callback: InheritedCallback, rejection: InheritedCallback): unknown };
inheritedCallback;
[inheritedCallback];
