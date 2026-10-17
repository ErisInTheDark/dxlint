declare const unsafe: any;
declare const list: any[];
declare function accept(value: number): void;
declare function tag(strings: unknown, first: number, middle: any, last: string): void;
declare class Box { constructor(value: number); }
tag`${unsafe}${1}${unsafe}`;
tag`${1}${unsafe}${"safe"}`;
accept(unsafe);
accept(...unsafe);
accept(...list);
new Box(unsafe);
accept(1);
export {};
