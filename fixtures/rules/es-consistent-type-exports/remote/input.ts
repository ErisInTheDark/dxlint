const T = 1;
type Alias = number;
export { T as 'fruit' } from './types';
export { T } from './values';
export { Alias } from './barrel';
export * from './types';
export * as Types from './type-barrel';
export * from './values';
export * from './missing';
