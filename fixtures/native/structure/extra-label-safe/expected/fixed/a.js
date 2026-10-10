loop: while (condition) { continue; break; }
choice: switch (value) { default: break; }
count: for (;;) { break; }
keys: for (const key in object) { continue; }
items: for (const item of list) { break; }
again: do { continue; } while (condition);
outer: while (condition) { inner: { break; } }
