# ESLint conservatism delta

`outcome`'s initial value is unused: every route to `finally` first writes `success` or `exception`. Reporting it intentionally differs from ESLint's conservative suppression around exception handlers.

`done`'s initial value is used: stopping the generator at `yield` enters `finally` without running either later assignment.
