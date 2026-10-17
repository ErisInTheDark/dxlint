declare class Callbacks {
  /** @deprecated */
  old: () => string;
  fresh: () => string;
}
declare const callbacks: Callbacks;
callbacks.old();
callbacks.fresh();
new Callbacks().fresh();
interface Handler {
  /** @deprecated */
  old: () => string;
}
declare const handler: Handler;
handler.old();
/** @deprecated */
declare class Old {}
new Old();
declare function overloaded(value: "fresh"): void;
/** @deprecated */
declare function overloaded(value: "old"): void;
overloaded("fresh");
overloaded("old");
Callbacks;
class MethodOverloads {
  a(value: "fresh"): void;
  /** @deprecated */
  a(value: "old"): void;
}
declare const withOverloads: MethodOverloads;
withOverloads.a("fresh");
withOverloads.a("old");
namespace assert {
  export function fail(message?: string): never;
  /** @deprecated */
  export function fail(actual: unknown, expected: unknown): never;
}
assert.fail("");
assert.fail(1, 2);
