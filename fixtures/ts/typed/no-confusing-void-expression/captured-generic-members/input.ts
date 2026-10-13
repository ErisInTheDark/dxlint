interface Phantom<T> {}
interface Concrete<T> { value: void }
interface Used<T> { value: T }
type Capture<T> = T extends infer U ? void : never;
declare function phantom(): Capture<Phantom<void>>;
declare function concrete(): Capture<Concrete<void>>;
declare function used(): Capture<Used<void>>;
const a = phantom();
const b = concrete();
const c = used();

interface Promise<T> { then(callback: (value: T) => void): void }
type Awaited<T> = T extends Promise<infer U> ? U : T;
declare function promise(): Promise<void>;
async function awaiting() { const value = await promise(); }
