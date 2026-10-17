export {};
interface Promise<T> { then(callback: (value: T) => unknown): unknown; }
declare const p: Promise<number>;
function promised(): Promise<number>;
function promised(flag: boolean): Promise<number>;
function promised(flag?: boolean): Promise<number> { return p; }
function mixed(): number | Promise<number>;
function mixed(flag: boolean): Promise<number>;
function mixed(flag?: boolean) { return flag ? p : 1; }
function dynamic(): number;
function dynamic(flag: boolean): any;
function dynamic(flag?: boolean): any { return p; }
function opaque(): number;
function opaque(flag: boolean): unknown;
function opaque(flag?: boolean): unknown { return p; }
function synchronous(): number;
function synchronous(flag: boolean): string;
function synchronous(flag?: boolean): number | string { return 1; }
function explicit(): number | Promise<number>;
function explicit(flag: boolean): Promise<number>;
function explicit(flag?: boolean): number | Promise<number> { return p; }
