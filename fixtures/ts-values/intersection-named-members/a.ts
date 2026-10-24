type Mapped<K extends keyof any, V> = { [P in K]: V };
type Num = { [key: number]: string | undefined };
type Str = { [key: string]: string | undefined };
type Mix<E extends number, V> = Mapped<E, V> & { [key: number]: V | undefined };
enum Kind { A, B }
declare const probeNum: Num;
declare const probeStr: Str;
declare const probeMix: Mix<Kind, string>;
declare const probeDirect: Mapped<Kind, string> & Num;
declare const probeExplicit: { then?: () => void } & Num;
probeNum;
probeStr;
probeMix;
probeDirect;
probeExplicit;
export {};
