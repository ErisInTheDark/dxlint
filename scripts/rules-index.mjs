// Rewrites docs/rules/README.md, the rule index, from the rule pages beside it,
// and each page's Rulesets row from rules/*.dx.
// usage: node scripts/rules-index.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const dir = path.join(root, "docs", "rules");
const rulesetDir = path.join(root, "rules");

// Each ruleset's `rule` lines (pattern, severity) and the rulesets it imports.
const rulesets = new Map(fs.readdirSync(rulesetDir).filter(file => file.endsWith(".dx")).sort().map(file => {
  const text = fs.readFileSync(path.join(rulesetDir, file), "utf8");
  const lines = [...text.matchAll(/^rule\s+([\w:*-]+)(?:\s+(hint|info|warn|error)\b)?/gm)].map(m => ({ pattern: m[1], severity: m[2] }));
  const imports = [...text.matchAll(/^import\s+rules:([\w-]+)/gm)].map(m => m[1]);
  return [file.slice(0, -".dx".length), { lines, imports }];
}));
const covers = (pattern, rule) => {
  const family = pattern.replace(/\*$/, "").replace(/:.*$/, "");
  return pattern.endsWith("-*") ? rule.startsWith(pattern.slice(0, -1)) : family === rule;
};
// `rules:x` (with the severity it sets, if any) for the rulesets naming
// `rule`, then the rulesets importing those.
function membership(rule) {
  const direct = [...rulesets].flatMap(([name, set]) => set.lines.filter(line => covers(line.pattern, rule)).slice(0, 1).map(line => ({ name, severity: line.severity })));
  const via = [...rulesets].filter(([, set]) => set.imports.some(imported => direct.some(d => d.name === imported))).map(([name]) => name);
  return [...direct.map(d => `\`rules:${d.name}\`${d.severity ? ` (${d.severity})` : ""}`), ...via.map(name => `\`rules:${name}\``)];
}

function page(file) {
  const rule = file.slice(0, -".md".length);
  const sets = membership(rule);
  const original = fs.readFileSync(path.join(dir, file), "utf8");
  const updated = original.replace(/^\| Rulesets \|.*\|$/m, `| Rulesets | ${sets.join(", ") || "none"} |`);
  if (updated !== original) fs.writeFileSync(path.join(dir, file), updated);
  const lines = updated.split(/\r?\n/);
  const cell = name => lines.find(line => line.startsWith(`| ${name} |`))?.split("|")[2]?.trim() ?? "";
  const summary = lines.slice(1).find(line => line.trim() && !line.startsWith("|") && !line.startsWith("#")) ?? "";
  const [pass, total] = cell("Parity").replace(/ corpus cases$/, "").split(" / ").map(Number);
  return {
    rule,
    sets: sets.filter(set => !set.startsWith("`rules:recommended`")).map(set => set.replace(/^`rules:([\w-]+)`.*$/, "$1")).join(", "),
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
- **Ruleset**: the ruleset that turns the rule on (\`rules:bugs\` as an error, the others as warnings); \`rules:recommended\` is \`bugs\`, \`suspicious\` and \`type-safety\`. No rule is in two rulesets.
- **Parity**: the share of the upstream rule's own test cases where dxlint gives the same result. ${perfect.length} of the ${measured.length} measured rules match every case; ${passing.toLocaleString("en")} of ${cases.toLocaleString("en")} cases match overall. Each page lists its known differences.

Pages are adapted from the documentation of [ESLint](https://eslint.org/docs/latest/rules/), [typescript-eslint](https://typescript-eslint.io/rules/), [ESLint Stylistic](https://eslint.style/rules) and [eslint-plugin-import](https://github.com/import-js/eslint-plugin-import), all MIT licensed.

| Rule | What it reports | Types | Fix | Ruleset | Parity |
| --- | --- | --- | --- | --- | --- |
${pages.map(p => `| [${p.rule}](${p.rule}.md) | ${p.summary} | ${p.types ? "yes" : ""} | ${p.fix} | ${p.sets} | ${parity(p)} |`).join("\n")}
`;
fs.writeFileSync(path.join(dir, "README.md"), text);
console.log(`${pages.length} rules, ${perfect.length}/${measured.length} at full parity, ${passing}/${cases} cases`);
