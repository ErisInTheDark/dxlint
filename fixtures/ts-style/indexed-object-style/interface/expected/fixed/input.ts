type Plain = Record<string, number>;
export type Generic<A = { value: string }, B = "interface"> = Readonly<Record<A, B>>;
declare type Ambient = Record<string, unknown>;
interface Inherited extends Base { [key: string]: unknown; }
export default interface Default { [key: string]: unknown; }
