export function moduleScope() { { let value = 1; use(value); } }
export const C = class { static { let value = 1; use(value); } };
