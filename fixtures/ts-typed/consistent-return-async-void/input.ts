interface Promise<T> { then(): T } interface Promise<T> { value: T }
declare function action(): void;
async function direct(flag: boolean): Promise<void> { if (flag) return action(); return; }
type Nested = Promise<Promise<void | undefined>>;
async function nested(flag: boolean): Nested { if (flag) return undefined; return; }
type Choice = Promise<void | number>;
async function choice(flag: boolean): Choice { if (flag) return 42; return; }
class Test { async method(flag: boolean): Promise<void> { if (flag) return action(); return; } }
async function required(flag: boolean): Promise<number> { if (flag) return 42; return; }
