interface Overloaded { (): void; (value: string): undefined; }
const overloaded = ((value => {}) as Overloaded)('') as undefined;
declare const known: () => number;
const result = known() as number;
declare const holder: { read(): number };
const property = holder.read() as number;
declare const mixed: { (): number; <T>(): T };
const mixedResult = mixed() as number;
declare const generic: <T>() => T;
const genericResult = generic() as number;
declare const dynamic: any;
const dynamicResult = dynamic() as number;
function wrapped(callback: () => number) { return callback() as number; }
