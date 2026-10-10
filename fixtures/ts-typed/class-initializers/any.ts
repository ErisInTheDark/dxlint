declare const x: { value: any }; if (typeof x.value === 'boolean') { x.value = x.value === true ? 13 : 0; }
export {};
