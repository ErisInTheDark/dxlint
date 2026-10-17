declare function nullable(s: string | null): void;
declare function required(s: string): void;
declare const s: string | null;
nullable(s!);
required(s!);
let missing: string;
nullable(missing!);
declare function opaque(value: any): void;
opaque(s!);
