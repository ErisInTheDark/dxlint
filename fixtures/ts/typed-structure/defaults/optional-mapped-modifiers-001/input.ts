type Partial<T> = { [P in keyof T]?: T[P] };
type Readonly<T> = { readonly [P in keyof T]: T[P] };
declare const partial: Partial<{ value: string }>;
declare const required: { value: string };
declare const locked: Readonly<{ value: string }>;
const { value: partialValue = "" } = partial;
const { value: requiredValue = "" } = required;
const { value: readonlyValue = "" } = locked;
