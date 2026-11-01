// Reports what it reads, then what it produces for its argument.
import { readFileSync } from "node:fs";
const [which] = process.argv.slice(2);
process.stdout.write(`dxlint-input data.txt\n${which}: ${readFileSync("data.txt", "utf8")}`);
