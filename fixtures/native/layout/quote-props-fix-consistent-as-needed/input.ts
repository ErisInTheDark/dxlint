const redundant = { "foo": 1, bar: 2, "\u0062az": 3 };
const necessary = { "not-valid": 1, foo: 2 };
interface Shape { readonly "foo"?: string; "bar"(): void }
enum Choice { first, "second" }
import data from "data" with { "type": "json" };
