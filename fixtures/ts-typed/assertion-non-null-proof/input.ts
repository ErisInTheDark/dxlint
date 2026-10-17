function stringValue<T extends string>(value: T) { return value!; }
function numberValue<T extends number>(value: T) { return value!; }
function nullableValue<T extends string | null>(value: T) { return value!; }
function unknownValue<T>(value: T) { return value!; }
declare const text: string;
const truthyText = (text || undefined) as string;
declare const number: number;
const truthyNumber = (number || undefined) as number;
declare const literal: "yes" | undefined;
const sameLiteral = literal as "yes";
const widerLiteral = literal as string;
const otherLiteral = literal as "no";
declare const dynamic: any;
const unknownCast = dynamic as string;
function anyIntersection(value: any & string) { return value!; }
function missingIntersection(value: Missing & string) { return value!; }
