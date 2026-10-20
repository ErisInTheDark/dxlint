interface Thenable<T> { then(onfulfilled: (value: T) => void): void }
interface Defaulted<T = number> { then(onfulfilled: (value: T) => void): void }
interface Merged<T> { then(onfulfilled: (value: T) => void): void }
interface Merged<T = number> { extra?: T }

declare const bare: Thenable;
declare const typed: Thenable<number>;
declare const defaulted: Defaulted;
declare const merged: Merged;

if (bare) {}
if (typed) {}
if (defaulted) {}
if (merged) {}
