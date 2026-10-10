const divisor = 4;
const defaultDelay = 200 / divisor;
declare function clone<T>(value: T): T;
export class C { static defaults = { delay: defaultDelay }; options = clone(C.defaults); }
