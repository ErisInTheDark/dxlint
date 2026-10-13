type VoidAlias = void;
type UnknownMatch = (() => unknown) extends (() => unknown) ? void : number;
type AnyMatch = (() => any) extends (() => any) ? void : number;
type NeverReject = (() => number) extends (() => never) ? number : void;
type VoidAccept = (() => number) extends (() => void) ? void : number;
type AliasAccept = (() => number) extends (() => VoidAlias) ? void : number;
type UndefinedReject = (() => number) extends (() => undefined) ? number : void;
declare function unknownMatch(): UnknownMatch;
declare function anyMatch(): AnyMatch;
declare function neverReject(): NeverReject;
declare function voidAccept(): VoidAccept;
declare function aliasAccept(): AliasAccept;
declare function undefinedReject(): UndefinedReject;
const a = unknownMatch();
const b = anyMatch();
const c = neverReject();
const d = voidAccept();
const e = aliasAccept();
const f = undefinedReject();

interface Promise<T> { then(callback: (value: T) => unknown): unknown }
type Awaited<T> = T extends Promise<infer U> ? U : T;
declare function promise(): Promise<void>;
async function awaiting() { const value = await promise(); }

type UnknownAlias = unknown;
type AliasUnknownMatch = (() => unknown) extends (() => UnknownAlias) ? void : number;
declare function aliasUnknownMatch(): AliasUnknownMatch;
const g = aliasUnknownMatch();
type NeverAlias = never;
type AliasNeverReject = (() => number) extends (() => NeverAlias) ? number : void;
declare function aliasNeverReject(): AliasNeverReject;
const h = aliasNeverReject();
