declare function takes(value: number): void;
declare function acceptsNull(value: number | undefined): void;
let assigned: number | undefined;
assigned = 1;
takes(assigned!);
let missing: number | undefined;
acceptsNull(missing!);
