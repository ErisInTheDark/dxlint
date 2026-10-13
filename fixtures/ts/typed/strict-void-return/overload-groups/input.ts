declare function first(cb: (arg: string) => () => void): void;
declare function first(cb: (arg: number) => () => boolean): void;
first((arg: number) => { return booleanCallback; });

declare function swapped(cb: (arg: number) => () => boolean): void;
declare function swapped(cb: (arg: string) => () => void): void;
swapped((arg: number) => { return booleanCallback; });

function booleanCallback() { return true; }

declare const runtime: (() => true) | (() => false);
if (runtime()) {}

declare const certain: () => true;
if (certain()) {}
