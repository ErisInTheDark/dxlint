declare const chances: Map<string, number>;
declare const view: ReadonlyMap<string, number>;
declare const seen: Set<string>;
declare const cache: WeakMap<object, number>;
class Registry extends Map<string, number> {}
declare const registry: Registry;
Object.entries(chances);
Object.keys(view);
Object.values(seen);
Object.keys(cache);
Object.entries(registry);
Object.entries(new Map<string, number>());
for (const key in seen) {}
declare const record: Record<string, number>;
declare const either: Map<string, number> | Record<string, number>;
declare const loose: any;
declare const args: Map<string, number>[];
Object.entries(record);
Object.entries(either);
Object.keys(loose);
Object.entries(chances.entries());
Object.keys(...args);
for (const key in record) {}
function shadowed(Object: { keys(o: unknown): string[] }) {
  Object.keys(chances);
}
