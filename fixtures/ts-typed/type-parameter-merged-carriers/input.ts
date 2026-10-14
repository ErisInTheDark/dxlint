declare function take<T>(value: T[]): void;
declare function takeReadonly<T>(value: readonly T[]): void;
declare function give<T>(): T[];
declare function giveReadonly<T>(): readonly T[];
declare function relate<T>(value: T[]): T;
declare function pair<T>(value: [T, T]): void;
export {};
