// One run for every seal: a section per argument list.
import { readFileSync } from "node:fs";
const data = readFileSync("data.txt", "utf8");
process.stdout.write(`dxlint-input data.txt\ndxlint-payload alpha\nalpha: ${data}dxlint-payload beta\nbeta: ${data}dxlint-error gamma\nno generator named gamma\n`);
