/** @deprecated use fresh */
export function old(): number { return 1; }
export function fresh(): number { return 2; }
/** @deprecated */
export function retired(): number { return 3; }
/** @deprecated use FreshClass */
export default class OldClass {}

export class FreshClass {}
/** @deprecated use FreshType */
export interface OldType { old: true; }
export interface FreshType { fresh: true; }
/** @deprecated use freshValue */
export const oldValue = 1;
export const freshValue = 2;
/** @deprecated use FreshEnum */
export enum OldEnum { A }
export enum FreshEnum { A }