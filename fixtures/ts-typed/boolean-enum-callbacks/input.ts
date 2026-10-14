enum Numeric { Zero = 0, One = 1 }
enum Text { Empty = "", Value = "value" }
declare const numeric: Numeric.Zero | null;
declare const text: Text.Empty | null;
declare const plain: number | null;
declare const items: number[];
items.filter(() => numeric);
items.filter(() => text);
items.filter(() => plain);
items.filter(() => true);
