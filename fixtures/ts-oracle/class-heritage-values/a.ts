interface Merged {
	merged?: string;
}
class Merged {
	own = 1;
}
declare const merged: Merged;
merged;
merged.merged;
merged.own;
new Merged().merged;
new Merged().own;

interface Implemented {
	implemented?: string;
}
class Implementer implements Implemented {
	own = 1;
}
declare const implementer: Implementer;
implementer;
implementer.own;
