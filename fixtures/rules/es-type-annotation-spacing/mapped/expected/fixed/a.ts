type Add<K extends string> = { [P in K]+?: number };
type Remove<K extends string> = { [P in K]-?: number };
