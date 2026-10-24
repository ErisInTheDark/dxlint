# no-sync

Reports calls to `Sync`-suffixed methods inside functions. Synchronous I/O can block other work; top-level calls are allowed.

| | |
| --- | --- |
| Upstream | [`no-sync`](https://eslint.org/docs/latest/rules/no-sync) |
| Needs types | no |
| Fix | none |
| Rulesets | none |

## Enable

```dx
import .ts
rule no-sync
```

## Examples

Incorrect:

```ts
function load() {
  return fs.readFileSync(somePath);
}
```

Correct:

```ts
function load() {
  return fs.promises.readFile(somePath);
}
fs.readFileSync(somePath);
```

## Differences from upstream

- dxlint always allows top-level calls. ESLint's `allowAtRootLevel` option is not exposed; dxlint behaves like that option is `true`.
- Matching is based on the `Sync` name suffix, not the actual API. Passing a bare `Sync`-suffixed identifier as an argument inside a function is also reported.
- No corpus parity result is recorded for this rule.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-sync) documentation (MIT).
