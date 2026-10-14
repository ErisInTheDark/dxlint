declare const maybe: string | null | undefined;
const a = maybe!;
const b = maybe!;
const c = (maybe || undefined)!;
declare const definite: string;
const d = definite as string;
const e = maybe as /* keep */ string;
