function outer() {
  function inner() {
    one(); two();
  } three();
  if (ready)
    four();
  if (ready) five();
  if (ready) { six(); seven(); }
  eight();
} nine();
