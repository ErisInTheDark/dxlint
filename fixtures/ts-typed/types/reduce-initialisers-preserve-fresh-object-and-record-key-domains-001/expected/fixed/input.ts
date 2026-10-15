declare const items: number[];
items.reduce((sum, item) => sum, {ok: 1, extra: 2} as {ok: number});
items.reduce<{ok: number}>((sum, item) => sum, {ok: 1});
items.reduce<Record<string, number>>((sum, item) => sum, {first: 1, second: 2});
items.reduce((sum, item) => sum, {ok: 1, extra: 2} as Record<"ok", number>);
items.reduce((sum, item) => sum, {ok: "bad"} as Record<string, number>);
