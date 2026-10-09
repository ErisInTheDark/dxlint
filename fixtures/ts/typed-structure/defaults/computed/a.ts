declare const flag: boolean; const { value = 'fallback' } = flag ? { ['value']: 'left' } : { [`value`]: 'right' };
