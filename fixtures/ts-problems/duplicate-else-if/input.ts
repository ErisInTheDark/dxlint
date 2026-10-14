if (a) {} else if (a) {}
if (a && b) {} else if (b && a) {}
if (a) {} else if (a && b) {}
if (a && b) {} else if (a) {}
if (a) {} else if (b) {} else if (a || b) {}
if (a) {} else if (a || b) {}
if (a || b) {} else if (b) {}
if ((a || b) && c) {} else if (c && (b || a)) {}
if (a || b) {} else if ((a || b) && c) {}
if (a && b) {} else if ((a && b) || c) {}
if (a) {} else { if (a) {} }
if (a) { if (a) {} }
if (a) {} if (a) {}
if (obj.x) {} else if (obj /* gap */ . x) {}
if ((a)) {} else if (((a))) {}
if (a + b) {} else if (ab) {}
if ("a b") {} else if ("ab") {}
if (1) {} else if (0x1) {}
if (a && (b || c)) {} else if ((c || b) && a) {}
if (a) {} else if (b) {} else if ((a || b) && c) {}
if (a || b) {} else if (c || d) {} else if ((a && x) || (d && y)) {}
if (a && b) {} else if ((b && a) || c) {}
