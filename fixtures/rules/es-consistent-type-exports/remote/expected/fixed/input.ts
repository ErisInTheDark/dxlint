const T = 1;
type Alias = number;
export type { T as 'fruit' } from './types';
export { T } from './values';
export type { Alias } from './barrel';
export type * from './types';
export type * as Types from './type-barrel';
export * from './values';
export * from './missing';
