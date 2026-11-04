import { both } from "../both";
import { chain } from "../chain";
import required = require("../required");

export const upgrade = both + chain + required.required;
export const later = import("../dynamic");
export let typed: import("../typed").Typed;
// import { live } from "../commented";
export const text = "import { live } from '../quoted'";
