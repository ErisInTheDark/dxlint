import { Emitter } from "./events";
@decorate
export default class Store<T = string> extends Emitter.Host<number> {
  value!: T;
  save(): Task<T> { return 0 as never }
  stop(): never { throw 0 }
}
export interface Box { store: Store<number>; }
interface Task<R> { then(ok: (r: R) => void): void; }
