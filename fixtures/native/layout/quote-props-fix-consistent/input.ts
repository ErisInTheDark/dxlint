const mixed = { foo: 1, "bar": 2, 1e3: 3 };
const bare = { foo: 1, bar: 2 };
interface Shape { readonly foo?: string; "bar"(): void }
enum Choice { first, "second" }
import data from "data" with { type: "json", "mode": "strict" };
