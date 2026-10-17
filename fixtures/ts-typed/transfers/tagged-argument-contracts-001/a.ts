declare function tag(strings: unknown, value: number): void;
declare function loose(strings: unknown, value: unknown): void;
declare const unsafe: any;
tag`value: ${unsafe}`;
tag`value: ${1}`;
loose`value: ${unsafe}`;
tag(undefined, unsafe);
export {};
