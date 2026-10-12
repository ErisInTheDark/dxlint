type Type = string;
interface Face {}
namespace Types { export type T = number; }
namespace Values { export const x = 1; }
const value = 1;
export { Type, Face, Types };
export { Type, value, Values };
export type { value };
export { Type as Alias, value as Runtime };
