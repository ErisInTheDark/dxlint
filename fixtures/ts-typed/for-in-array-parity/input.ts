declare const array: number[] | { a: 1 };
for (const key in array) {}
for (const key in []) {}
function constrained<T extends any[]>(arr: T) { for (const key in arr) {} }
declare const indexed: { [key: number]: number; length: 1 };
for (const key in indexed) {}
declare const separate: { [key: number]: number } | { length: number };
for (const key in separate) {}
for (const key in 'text') {}
for (const key in { length: 1 }) {}
