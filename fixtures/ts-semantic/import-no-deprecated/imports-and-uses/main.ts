import { old, fresh, retired, OldType, oldValue, OldEnum } from "./barrel";
import OldClass from "./defs";
import retiredDefault from "./defaults";
import { old as renamed } from "./defs";
import type { OldType as OldTypeAlias } from "./barrel";

old();
fresh();
retired;
oldValue;
OldEnum.A;
const keep: OldTypeAlias = { old: true };
const instance = new OldClass();
renamed();
retiredDefault();

function inner() {
	const old = () => 1;
	return old();
}

/** @deprecated */
function localOld() {}
localOld();
export { retired };