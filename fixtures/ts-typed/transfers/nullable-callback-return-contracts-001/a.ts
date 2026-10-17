declare function accept(value: null | (() => any)): void;
declare function acceptUnknown(value: null | (() => unknown)): void;
declare const value: any;
accept(() => value);
accept(function () { return value; });
accept((): any => value);
acceptUnknown(() => value);
accept(() => 1);
export {};
