interface Promise<T> { then(fulfilled: (value: T) => unknown): unknown; }
type Numeric = number;
type PromisedNumber = Promise<number> & Numeric;
declare const awaitableNumber: PromisedNumber;
declare const awaitableString: string & Promise<number>;
declare const awaitableBoolean: boolean & Promise<number>;
declare const awaitableBigint: bigint & Promise<number>;
declare const awaitableSymbol: symbol & Promise<number>;
declare const awaitableLiteral: 1 & Promise<number>;
const objectValue = { brand: true };
declare const awaitableObject: typeof objectValue & Promise<number>;
enum Status { Ready = 1 }
declare const awaitableEnum: Status & Promise<number>;
declare const awaitableEnumObject: typeof Status & Promise<number>;
interface AwaitableBase { then(fulfilled: (value: number) => unknown): unknown; }
interface AwaitableDerived extends AwaitableBase { brand: true; }
declare const awaitableInherited: AwaitableDerived;
declare const ordinaryNumber: number;
declare const ordinaryString: string;
declare const ordinaryBoolean: boolean;
declare const ordinaryBigint: bigint;
declare const ordinarySymbol: symbol;
declare const ordinaryBrand: number & { brand: true };
const ordinaryObject = objectValue;
const ordinaryEnum = Status;
const ordinaryEnumValue = Status.Ready;
declare const ordinaryEnumType: Status;
const ordinaryThenValue = { then: 1 };
declare const unresolvedKey: string;
const uncertainObject = { [unresolvedKey]: 1 };
declare const impossible: number & string;
awaitableNumber;
awaitableString;
awaitableBoolean;
awaitableBigint;
awaitableSymbol;
awaitableLiteral;
awaitableObject;
awaitableEnum;
awaitableEnumObject;
awaitableInherited;
ordinaryNumber;
ordinaryString;
ordinaryBoolean;
ordinaryBigint;
ordinarySymbol;
ordinaryBrand;
ordinaryObject;
ordinaryEnum;
ordinaryEnumValue;
ordinaryEnumType;
ordinaryThenValue;
uncertainObject;
impossible;
function constrainedNumber<T extends number>(ordinary: T) { ordinary; }
function constrainedString<T extends string>(ordinary: T) { ordinary; }
function chainedConstraint<R extends number, T extends R>(ordinary: T) { ordinary; }
function unconstrained<T>(uncertain: T) { uncertain; }
function constrainedPromise<T extends Promise<number>>(awaitable: T) { awaitable; }
export {};
