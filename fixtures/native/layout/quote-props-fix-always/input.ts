const object = { foo: 1, \u0062ar: 2, 1e3: 3, "kept": 4, [key]: 5, shorthand };
interface Shape { readonly foo?: string; bar(): void }
type Alias = { baz: number };
enum Choice { first, "second" }
import data from "data" with { type: "json" };
