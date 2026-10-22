declare const s: string;
s;
declare const pn: Promise<number>;
pn;
declare const aw1: Awaited<string>;
aw1;
declare const aw2: Awaited<number>;
aw2;
declare const aw3: Awaited<null>;
aw3;
declare const aw4: Awaited<{ x: 1 }>;
aw4;
declare const aw5: Awaited<Promise<string>>;
aw5;
declare const aw6: Awaited<Promise<number>>;
aw6;
declare const aw7: Awaited<MyThenable<string>>;
aw7;
interface L3 { q: number; }
interface L2 { then(onfulfilled: (v: L3) => void): any; }
interface L1 { then(onfulfilled: (v: L2) => void): any; }
type Aw2<T> = T extends null | undefined
  ? T
  : T extends object & { then(onfulfilled: infer F, ...args: infer _): any }
    ? (F extends ((value: infer V, ...args: infer _) => any) ? Aw2<V> : never)
    : T;
declare const chain: Aw2<L1>;
chain;
