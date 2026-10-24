type Arguments<X extends any[]> = X["length"] extends 0 ? [] : X["length"] extends 1 ? [X[0]] : never;
declare const pair: [string, number];
declare const optional: [string, number?];
declare const empty: [];
declare const one: Arguments<[string]>;
declare const none: Arguments<[]>;

pair.length;
optional.length;
empty.length;
one;
none;
export {};
