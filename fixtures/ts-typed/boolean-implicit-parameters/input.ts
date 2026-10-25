x => !x;
function plain(x) { return !x; }
const object = { method(x) { return !x; } };
(x?) => !x;
(x: any) => !x;
(x: boolean) => !x;
const contextual: (x: boolean) => boolean = x => !x;
declare function use(callback: (x: boolean) => boolean): void;
use(x => !x);
(x = 1) => !x;
(...values: boolean[]) => !values[0];
({ value }) => !value;
(x: unknown) => !x;
