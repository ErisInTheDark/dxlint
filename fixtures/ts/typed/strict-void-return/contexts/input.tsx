declare const bad: () => number;
const assigned: () => void = bad;
declare let target: () => void;
target = () => 1;
const object: { cb(): void } = { cb() { return 1; } };
const array: (() => void)[] = [bad];
function factory(): () => void { return bad; }
const nested: () => () => void = () => () => 1;
interface Base { cb(): void; }
class Derived implements Base { cb() { return 1; } }
declare function Component(props: { cb: () => void }): unknown;
const element = <Component cb={() => 1} />;
const ignored = { ["cb"]() { return 1; } };
declare let optional: { cb?: () => void };
optional.cb = () => { return 1; };
declare let mixed: (() => void) | boolean;
mixed ||= () => 1;
declare let maybe: (() => void) | null;
maybe ??= () => 1;
const mixedObject: { cb: (() => void) | number } = { cb: () => 1 };
const mixedArray: ((() => void) | false)[] = [false, bad, () => 1];
declare function OptionalComponent(props: { cb?: () => void }): unknown;
const optionalElement = <OptionalComponent cb={function* () {}} />;
const asyncElement = <OptionalComponent cb={async function* () {}} />;
interface AccessorBase { cb(): void; }
class Accessor implements AccessorBase { get cb() { return 1; } set cb(value: number) { return 1; } }
const choice: (() => void) | (() => unknown) = () => 1;
declare let nestedUnion: () => (() => void) | string;
nestedUnion = () => () => { return 1; };
mixed &&= () => 1;
