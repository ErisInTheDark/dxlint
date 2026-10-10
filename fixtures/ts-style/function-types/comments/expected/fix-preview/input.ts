// keep
type Α = () => string;
/** keep */
export type B = () => string;
type C = /* keep */ ((s: string) => number) | undefined;
type D = (x: /* inside */ string) => /* return */ number;
// trailing
type E = () => void;
