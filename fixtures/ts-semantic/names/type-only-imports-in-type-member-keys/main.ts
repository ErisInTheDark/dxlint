// Type-only imports read inside type syntax are used.
import type { PathType, Kind, settings, Unused } from "./keys";

interface Paths {
	[PathType.Map]: string;
}

type Leaves = { [Kind.Leaf]: number };

type Settings = typeof settings;

export type { Paths, Leaves, Settings };
