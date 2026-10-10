declare function take<T>(value: T): T;
interface Mixed { days: number; ready: boolean; }
take<Mixed>({ days: 0, ready: true });
interface Numeric { days: number; }
take<Numeric>({ days: 0 });
declare const mixed: Mixed;
take<Mixed>(mixed);
take<boolean>(true);
declare function tuple<T extends readonly unknown[]>(value: T): T;
tuple<[1, 2]>([1, 2]);
declare function returnsTrue(): true;
interface BooleanField { ready: boolean; }
take<BooleanField>({ ready: returnsTrue() });
take<BooleanField>({ ready: true as true });
const fresh = true;
const uncontextualised = { ready: fresh };
take<BooleanField>(uncontextualised);
class Holder { readonly copied = fresh; readonly fixed: true = true; }
declare const holder: Holder;
const readonlyFresh = { ready: holder.copied };
take<BooleanField>(readonlyFresh);
take<BooleanField>({ ready: holder.fixed });
export {};
import component, { E } from "./base";
component<Mixed>(E.A, { days: 0, ready: true });
