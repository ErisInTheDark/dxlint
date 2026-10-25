type A = string;
interface B {}
const value = 1;
export { A, B as Alias, type value as Existing, value };
export { type A, A };
