type Dictionary<K extends string, V> = { [P in K]: V };
type Reducer = { reduce: (callback: (arg: any) => any, initial: any) => any };
declare const overridden: number[] & Reducer;
declare const array: number[];
overridden.reduce(a => a, {} as Dictionary<string, number>);
array.reduce(a => a, {} as Dictionary<string, number>);
