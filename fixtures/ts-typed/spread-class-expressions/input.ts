const Ctor = class {};
const copied = { ...Ctor };
const direct = { ...(class { static value = 1 }) };
declare const ordinary: { field: number };
const safe = { ...ordinary };
