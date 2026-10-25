import type { Already , A} from "same";
import { Value } from 'same';
import { B, Other } from 'same';
const value = Value() + Other();
type Result = Already | A | B;
