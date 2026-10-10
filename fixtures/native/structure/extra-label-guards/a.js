commented: while (condition) { break /* keep */ commented; }
continued: while (condition) { continue /* keep */ continued; }
outer: while (condition) { inner: while (condition) { break outer; continue outer; } }
block: { break block; }
ordinary: while (condition) { break; }
