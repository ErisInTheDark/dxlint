type A = string;
interface B {}
const value = 1;
export { type A, type B as Alias, type value as Existing, value };
export type { A, A };
