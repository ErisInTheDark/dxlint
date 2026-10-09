const a = Object.assign(
  {
    value: foo //
  },
  bar
);
const b = Object.assign(({ a: 1 }), (bar));
