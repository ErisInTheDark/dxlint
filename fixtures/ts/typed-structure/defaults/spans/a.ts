declare const mixed: [boolean, ...number[], string];
const [flag, count = 0, name = 'x'] = mixed;

declare const commands: [string, ...string[]];
const [cmd, arg = 'run'] = commands;

declare const tail: [...string[], string | undefined];
const [first = 'fallback'] = tail;

declare const fixed: [number, number?];
const [required = 1, optional = 1] = fixed;

function take({ present = 0 }: { present: number }) {}
