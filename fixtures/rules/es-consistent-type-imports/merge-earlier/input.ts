import type { Already } from "same";
import { A, Value } from 'same';
import { B, Other } from 'same';
const value = Value() + Other();
type Result = Already | A | B;
