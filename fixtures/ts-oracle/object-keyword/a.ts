interface HasP { p: number; }
interface Thenable { then(onfulfilled: (value: number) => void): any; }
type IsObject<T> = T extends object ? 1 : 0;
declare const o1: IsObject<string>;
o1;
declare const o2: IsObject<number>;
o2;
declare const o3: IsObject<{ p: number }>;
o3;
declare const o4: IsObject<HasP>;
o4;
declare const o5: IsObject<(v: number) => void>;
o5;
declare const o6: IsObject<number[]>;
o6;
declare const o7: IsObject<null>;
o7;
declare const o8: IsObject<{}>;
o8;
declare const o9: IsObject<Thenable>;
o9;
