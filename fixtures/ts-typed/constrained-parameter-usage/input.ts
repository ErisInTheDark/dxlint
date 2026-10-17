interface Lengthy { length: number }
function direct<T extends { length: number }>(value: T): number { return value.length; }
function named<T extends Lengthy>(value: T): number { return value.length; }
declare function twice<T extends Lengthy>(a: T, b: T): number;
declare function returns<T extends Lengthy>(a: T): T;
declare function intersection<T>(a: T & { value: T }): void;
declare function related<T, U extends { value: T }>(a: U, b: T): void;
