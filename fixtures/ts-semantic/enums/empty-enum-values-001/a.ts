enum Empty {}
const enum ConstEmpty {}
enum Populated { First }
declare const empty: Empty;
declare const constant: ConstEmpty;
declare const populated: Populated;
const statics = Empty;
empty;
constant;
populated;
export {};
