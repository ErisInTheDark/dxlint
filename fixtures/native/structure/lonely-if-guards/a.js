if (a) {} else { /* keep */ if (b) {} }
if (a) {} else { if (b) {} /* keep */ }
if (a) {} else { if (b) use(b) } next();
if (a) {} else { if (b) use(b)
}
[1].forEach(use);
if (a) {} else { if (b) use(b)
}
`template`;
if (a) {} else { if (b) value++
}
next();
if (a) if (b) {} else { if (c) {} } else {}
if (a) {} else { if (b) {} ; }
