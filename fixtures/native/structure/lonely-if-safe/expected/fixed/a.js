if (a) {} else if (b) {}
if (a) {} else if (b) {}
if (a) {} else /* keep */ if (b) {}
if (a) {} else if /* keep */ (b) { use(b); }
if (a) {} else if (b) use(b); next();
if (a) {} else if (b) use(b)
next();
if (a) {} else if (b) value++;
next();
if (a) {} else if (b) {
    use(b);
  } else if (c) {
    use(c);
  } else {
    use(d);
  }
