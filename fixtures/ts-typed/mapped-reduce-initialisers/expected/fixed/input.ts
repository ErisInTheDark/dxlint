type Dictionary<K extends string, V> = { [P in K]: V };
declare const items: number[];
items.reduce<Dictionary<string, number>>((acc, item) => acc, {});
items.reduce<Dictionary<"ok", number>>((acc, item) => acc, {ok: 1});
items.reduce((acc, item) => acc, {ok: "bad"} as Dictionary<string, number>);
items.reduce((acc, item) => acc, {ok: 1, extra: 2} as Dictionary<"ok", number>);
