declare function setHandler(handler: (action: number, position: { x: number }) => void): void;
setHandler((action, position) => position.x);

declare function each<T>(items: T[], run: (item: T) => void): void;
each([1, 2], item => item + 1);
each(["a"], item => item.length);

interface Failure { message: string }
declare function later(handler: (reason: Failure) => void): void;
later(reason => reason.message);
