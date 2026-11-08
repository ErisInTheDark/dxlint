# dxlint

let you and your agents own your linter

```dx
import .ts
import rules:recommended

rule es-eqeqeq error
rule es-no-floating-promises
```

```
$ dxlint
warn: Promises must be awaited, end with a call to .catch, end with a call to .then with a rejection handler or be explicitly marked as ignored with the `void` operator. [no-floating-promises]
  --> src/save.ts:4:3
  |
4 |   writeFile(path, data);
  |   ^^^^^^^^^^^^^^^^^^^^^^
```

## another linter? ugh...

- **it's over 200× faster than eslint+typescript-eslint.** because apples to apples requires eslint *without* its caching enabled, and dxlint's cache is intrinsic. (even without that, it's also twice as fast and significantly less memory hungry)
- **and it has almost perfect eslint parity.** 99.4% of eslint's and typescript-eslint's test cases give the same result. type-aware rules read your `tsconfig.json` *without running typescript*
- **everything is overrideable, you own it all.** built-in rules, and even the typescript parser, are `.dx` files: override them, tweak them, extend the grammar with syntax of your own, or describe a whole new language
- **plus a high quality vscode extension too!** extremely fast, with diagnostics, with a proper `.dx` lsp

## getting started

```sh
npm install --save-dev @inthedark/dxlint
```

put a `.dx` at your project root:

```dx
import .ts
import rules:recommended

ignore "dist" "src/generated/**"
```

| command | |
| --- | --- |
| `dxlint [paths]` | lint the project (respects `.gitignore`) |
| `dxlint --fix` | apply fixes |
| `dxlint rules` | list every rule with its options |
| `dxlint describe <name>` | explain a rule, option or anything else in the config |

exit code 1 means errors were found, 2 means the config is broken.&nbsp;

don't forget to add `.dxcache` to `.gitignore`!!!

for more on the basics: [docs/basic.md](docs/basic.md) and [the rule list](docs/rules/README.md).

## the language

a rule is a few lines of `.dx`: where to look, when it is wrong, and optionally the fix

```dx
import .ts

in ts_call.callee rule no-alert-calls error "use the notification service instead of {text}" {
  when text == "alert"
}
```

[docs/advanced.md](docs/advanced.md) covers the whole language, from rules over the built-in syntax trees to defining your own languages. [docs/ts.md](docs/ts.md) covers what the TypeScript library understands: scopes, modules, types and narrowing.

## performance

a project with 2,161 .ts files and its complex custom eslint config ported, type-aware rules included. eslint 9.39 with typescript-eslint 8.58, wall time, measured in one session on a 16-core machine:

| one core (`--jobs 1`) | ESLint | dxlint |
| --- | --- | --- |
| first run | 143 s | 58 s |
| cached run, nothing changed | 2.5 s | 2.0 s |
| peak memory (working set) | 3.6 GB | 0.60 GB |

| every core (dxlint's default; ESLint `--concurrency auto`) | ESLint | dxlint |
| --- | --- | --- |
| first run | 119 s | 11 s |
| cached run, nothing changed | 3.0 s | 1.5 s |
| peak memory (working set) | 18 GB | 1.07 GB |

and remember: the cached rows aren't a fair comparison because [eslint --cache doesn't work with typescript-eslint](https://typescript-eslint.io/troubleshooting/faqs/eslint/?utm_source=chatgpt.com#can-i-use-eslints---cache-with-typescript-eslint)! eslint's cache skips files whose own text didn't change, even when a type they use did, while dxlint's cache tracks what each file read&nbsp;

eslint's threads each build their own typescript program, so more cores mostly buy it memory; dxlint doesn't rely on typescript, so its work splits across cores
