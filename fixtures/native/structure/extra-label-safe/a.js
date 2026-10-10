loop: while (condition) { continue loop; break loop; }
choice: switch (value) { default: break choice; }
count: for (;;) { break count; }
keys: for (const key in object) { continue keys; }
items: for (const item of list) { break items; }
again: do { continue again; } while (condition);
outer: while (condition) { inner: { break outer; } }
