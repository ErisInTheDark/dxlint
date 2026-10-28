// Removing any of these type arguments changes the program; typescript-eslint
// reports none of them (checked against its own run).
export enum Sort { Name, Date }
export interface Options { quiet?: boolean }

// A numeric enum is not its `number` default.
export class Row<S extends number = number> { selected!: S }
export const row: Row<Sort> = new Row();
export function lookup<T extends string | number = number>(key: string): T { return key as never; }
lookup<Sort>("name");

// An intersection adding an optional member is not its base.
export class Manager<R extends Options = Options> { remove(options: R): R { return options; } }
export class Tamed extends Manager<Options & { tamed?: boolean }> {}

// One of several written type arguments cannot go.
export function choice<A extends unknown[], C extends number[]>(args: A, choices: C): [A, C] { return [args, choices]; }
choice<[string], [1, 2]>(["x"], [1, 2]);

// The default itself still goes.
export class Plain extends Manager<Options> {}
