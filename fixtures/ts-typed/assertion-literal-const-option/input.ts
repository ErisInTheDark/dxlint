const a = 'a' as const;
let preserved = 'a' as const;
const object = { value: 'a' } as const;
const array = ['a'] as const;
enum Choice { First, Second }
declare const member: Choice.First;
const selected = member as const;
const template = `a` as const;
