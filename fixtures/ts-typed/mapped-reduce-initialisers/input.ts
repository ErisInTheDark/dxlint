type Dictionary<K extends string, V> = { [P in K]: V };
declare const items: number[];
items.reduce((acc, item) => acc, {} as Dictionary<string, number>);
items.reduce((acc, item) => acc, {ok: 1} as Dictionary<"ok", number>);
items.reduce((acc, item) => acc, {ok: "bad"} as Dictionary<string, number>);
items.reduce((acc, item) => acc, {ok: 1, extra: 2} as Dictionary<"ok", number>);
