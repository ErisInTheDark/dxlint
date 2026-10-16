export class Dropdown<T extends string = never> { value!: T; }
export type Arguments<A extends unknown[]> = never;
export interface Description<A extends unknown[], AV extends unknown[] = Arguments<A>> { value: AV; }
