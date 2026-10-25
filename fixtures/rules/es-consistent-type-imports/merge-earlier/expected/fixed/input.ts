import type { Already , A, B} from "same";
import { Value } from 'same';
import { Other } from 'same';
const value = Value() + Other();
type Result = Already | A | B;
