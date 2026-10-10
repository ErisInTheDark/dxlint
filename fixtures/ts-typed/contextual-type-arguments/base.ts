export enum E { A }
interface Component<T, D = undefined> { get(): T | D; }
declare function component<T>(id: string, value: T): Component<T, T>;
declare function component<T>(id: string, value?: T): Component<T>;
declare function component<T>(id: E, value: T): Component<T, T>;
declare function component<T>(id: E, value?: T): Component<T>;
export default component;
