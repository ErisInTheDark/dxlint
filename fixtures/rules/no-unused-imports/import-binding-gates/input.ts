import usedDefault, { Used, mixedUnused, Remote as unusedAlias, DocOnly } from "./module";
import unusedDefault from "./module";
import * as unusedNamespace from "./module";
import { allUnusedA, allUnusedB } from "./module";
import type { UnusedType } from "./module";
import usedEquals = require("./module");
import unusedEquals = require("./module");
/** {@link DocOnly} */
void [usedDefault, Used, usedEquals];
const localUnused = 1;
