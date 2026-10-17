declare const maybe: string | null | undefined;
const present = maybe!;
declare const mixed: string | number | null;
const narrowed = mixed as string;
declare const nonnull: string;
const retained = nonnull as string;
