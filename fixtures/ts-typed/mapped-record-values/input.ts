type Entries<K extends string | number | symbol, V> = { [P in K]: V };
enum Choice { A, B }
declare const finite: Entries<"x" | "y", number>;
declare const numeric: Entries<Choice, string>;
declare const text: Entries<string, boolean>;
declare const ordinal: Entries<number, boolean>;
declare const symbolic: Entries<symbol, boolean>;
declare const dependent: { [P in "x" | "y"]: P };
declare const optional: { readonly [P in "x" | "y"]?: number };
declare const empty: Entries<never, number>;
declare const opaque: { [P in unknown]: number };
type Selection<T, K extends keyof T> = { [P in K]: T[P] };
type MutableSelection<T, K extends keyof T> = { -readonly [P in K]: T[P] };
declare const picked: Selection<{ readonly x?: number; y: string }, "x">;
declare const pickedRequired: Selection<{ x: number; y?: string }, "x">;
declare const pickedMutable: MutableSelection<{ readonly x: number }, "x">;
export {};
