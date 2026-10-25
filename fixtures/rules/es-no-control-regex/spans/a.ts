/\x11/;
/\x11/g;
/\x11[\x00]/;
RegExp("a\x11b");
new RegExp("\x00");
RegExp(process.env.PATTERN);
