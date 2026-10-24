// Rewrites docs/rules/README.md, the rule index, from the rule pages beside it.
// usage: node scripts/rules-index.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "docs", "rules");

function page(file) {
  const lines = fs.readFileSync(path.join(dir, file), "utf8").split(/\r?\n/);
  const cell = name => lines.find(line => line.startsWith(`| ${name} |`))?.split("|")[2]?.trim() ?? "";
  const summary = lines.slice(1).find(line => line.trim() && !line.startsWith("|") && !line.startsWith("#")) ?? "";
  const [pass, total] = cell("Parity").replace(/ corpus cases$/, "").split(" / ").map(Number);
  return {
    rule: file.slice(0, -".md".length),
    summary: summary.replace(/^(.*?[.!?])(\s.*)?$/, "$1").replaceAll("|", "\\|"),
    types: cell("Needs types") === "yes",
    fix: cell("Fix") === "none" ? "" : cell("Fix"),
    pass,
    total,
  };
}

const pages = fs.readdirSync(dir).filter(file => file.endsWith(".md") && file !== "README.md").sort().map(page);
const measured = pages.filter(p => p.total > 0);
const perfect = measured.filter(p => p.pass === p.total);
const cases = measured.reduce((sum, p) => sum + p.total, 0);
const passing = measured.reduce((sum, p) => sum + p.pass, 0);
const parity = p => !p.total ? "" : p.pass === p.total ? "100%" : `${(100 * p.pass / p.total).toFixed(1)}%`;

const text = `# Rules

Every rule dxlint ships, ${pages.length} in all. Turn one on with \`rule <name>\` after importing its language; see [basic.md](../basic.md).

- **Types**: the rule reads type information (from your \`tsconfig.json\`; TypeScript itself never runs).
- **Fix**: \`autofix\` rules are fixed by \`dxlint --fix\` and by the editor's quick fixes.
- **Parity**: the share of the upstream rule's own test cases where dxlint gives the same result. ${perfect.length} of the ${measured.length} measured rules match every case; ${passing.toLocaleString("en")} of ${cases.toLocaleString("en")} cases match overall. Each page lists its known differences.

Pages are adapted from the documentation of [ESLint](https://eslint.org/docs/latest/rules/), [typescript-eslint](https://typescript-eslint.io/rules/), [ESLint Stylistic](https://eslint.style/rules) and [eslint-plugin-import](https://github.com/import-js/eslint-plugin-import), all MIT licensed.

| Rule | What it reports | Types | Fix | Parity |
| --- | --- | --- | --- | --- |
${pages.map(p => `| [${p.rule}](${p.rule}.md) | ${p.summary} | ${p.types ? "yes" : ""} | ${p.fix} | ${parity(p)} |`).join("\n")}
`;
fs.writeFileSync(path.join(dir, "README.md"), text);
console.log(`${pages.length} rules, ${perfect.length}/${measured.length} at full parity, ${passing}/${cases} cases`);
