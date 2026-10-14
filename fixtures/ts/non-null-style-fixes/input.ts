declare const maybe: string | null | undefined;
const a = maybe as string;
const b = <string>maybe;
const c = (maybe || undefined) as string;
declare const definite: string;
const d = definite as string;
const e = maybe as /* keep */ string;
