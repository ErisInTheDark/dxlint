declare function objectArg(value: { cb: () => void }): void;
objectArg({ cb: () => 101 });

declare function callbackResult(callback: () => { nested: () => void }): void;
callbackResult(() => ({ nested: () => 103 }));

declare function nestedArrow(callback: () => () => void): void;
nestedArrow(() => () => 105);

enum Fruit { Apple }
function takesFruits(fruits: Fruit[]): void {}
takesFruits([1]);
takesFruits([Fruit.Apple]);

export {};
