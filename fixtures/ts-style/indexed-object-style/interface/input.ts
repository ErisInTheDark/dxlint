interface Plain { [key: string]: number; }
export interface Generic<A = { value: string }, B = "interface"> { readonly [key: A]: B; }
declare interface Ambient { [key: string]: unknown; }
interface Inherited extends Base { [key: string]: unknown; }
export default interface Default { [key: string]: unknown; }
