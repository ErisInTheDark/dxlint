declare const x: { value: number }; if (typeof x.value === 'boolean') { x.value = x.value === true ? 13 : 0; }
