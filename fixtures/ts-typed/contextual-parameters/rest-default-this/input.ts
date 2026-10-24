type Rest = (...values: number[]) => number;
export const rest: Rest = (...values) => values.length;

export const withDefault: (first: number, second: number) => number = (first, second = 2) => first + second;
export const plainDefault = (count = 0) => count;

type Receiver = (this: { value: number }, amount: number) => number;
export const receiver: Receiver = function (amount) { return this.value + amount; };
