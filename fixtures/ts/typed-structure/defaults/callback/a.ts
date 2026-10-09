interface Shape { callback: (value: number) => void; mixed: boolean | ((value: number) => void); } const object: Shape = { callback: (value = 1) => {}, mixed: (value = 1) => {} };
