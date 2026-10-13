interface Numeric { [index: number]: unknown; present: number }
interface Textual { [index: string]: unknown }
interface UnknownDomain { [index: unknown]: unknown }
interface MixedDomain { [index: number | symbol]: unknown }
interface Symbolic { [index: symbol]: unknown }
declare const uniqueKey: unique symbol;
declare const broadSymbol: symbol;
interface ComputedSymbol { [uniqueKey]: number }
interface UnresolvedSymbol { [broadSymbol]: number }
interface UnresolvedKey { [missing]: number }
interface LiteralKey { ["then"]: number }
