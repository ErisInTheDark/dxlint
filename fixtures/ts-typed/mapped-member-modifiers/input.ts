type Select<T, K extends keyof T> = { [P in K]: T[P] };
type Source = { readonly optional?: number; required: string };
declare const checkOptionalReadonly: Select<Source, "optional">;
declare const checkRequiredWritable: Select<Source, "required">;
declare const checkFullOptionalReadonly: { [P in keyof Source]: Source[P] };
type ReadonlyOptional<T, K extends keyof T> = { +readonly [P in K]+?: T[P] };
type RequiredMutable<T, K extends keyof T> = { -readonly [P in K]-?: T[P] };
declare const checkOptionalReadonlyAdded: ReadonlyOptional<{ optional: number }, "optional">;
declare const checkRequiredWritableRemoved: RequiredMutable<{ readonly required?: number }, "required">;
