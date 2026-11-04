export const used = 1;
export const aliased = 2;
export const dead = 3;
export const { destructured, other: renamed } = { destructured: 1, other: 2 };
export default class Thing {}

export function over(a: string): void;
export function over(a: number): void;
export function over(a: unknown) { return a; }

/** @public */
export function taggedFirst(a: string): void;
export function taggedFirst(a: number): void;
export function taggedFirst(a: unknown) { return a; }

export function taggedMiddle(a: string): void;
// @public
export function taggedMiddle(a: number): void;
export function taggedMiddle(a: unknown) { return a; }

export function taggedLast(a: string): void;
export function taggedLast(a: number): void;
/* @public */
export function taggedLast(a: unknown) { return a; }

// @publication is another tag, whole tags only
export const notTheTag = 4;

export class Merged {}
export namespace Merged { export const inner = 1; }

/** @public */
export class TaggedMerged {}
export namespace TaggedMerged { export const inner = 1; }
