const object = { "foo": 1, "bar": 2, "1000": 3, "kept": 4, [key]: 5, shorthand };
interface Shape { readonly "foo"?: string; bar(): void }
type Alias = { "baz": number };
enum Choice { "first", "second" }
import data from "data" with { "type": "json" };
