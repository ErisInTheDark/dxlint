type Type = string;
interface Face {}
namespace Types { export type T = number; }
namespace Values { export const x = 1; }
const value = 1;
export type { Type, Face, Types };
export type { Type };
export { value, Values };
export type { value };
export type { Type as Alias };
export { value as Runtime };
