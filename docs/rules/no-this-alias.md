# no-this-alias

Reports variables used to alias `this`. Arrow functions can retain `this` without a separate alias.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-this-alias`](https://typescript-eslint.io/rules/no-this-alias) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 10 / 10 corpus cases |

## Enable

```dx
import .ts
rule no-this-alias
```

## Examples

Incorrect:

```ts
class Worker {
  start() {
    const self = this;
    setTimeout(function () { self.doWork(); });
  }
  doWork() {}
}
```

Correct:

```ts
class Worker {
  start() {
    setTimeout(() => { this.doWork(); });
  }
  doWork() {}
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowDestructuring` | `boolean` | `true` | Allow declarations such as `const { props } = this`. |
| `allowedNames` | `string[]` | `[]` | Variable names allowed to alias `this`. |

```dx
rule no-this-alias {
  set allowDestructuring false
  set allowedNames ["self"]
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-this-alias` | Variable declarations initialised with `this`. |
| `no-this-alias:assignment` | Assignments of `this` to existing bindings. |

`rule no-this-alias` turns on every variant; `rule no-this-alias:assignment off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-this-alias) documentation (MIT).
