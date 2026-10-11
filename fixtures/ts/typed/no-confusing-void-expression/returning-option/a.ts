declare function v(): void;
declare function contextual(callback: () => void): void;
const explicit = (): void => v();
contextual(() => v());
function plain() { return v(); }
const value = v();
type VoidAlias = void;
const alias = (): VoidAlias => v();
const union = (): void | number => v();
const assigned: () => void = function () { return v(); };
const nested: () => () => void = () => () => v();
const notVoid = (): unknown => v();
const opaque: unknown = () => v();
function outer(): void { function inner() { return v(); } }
