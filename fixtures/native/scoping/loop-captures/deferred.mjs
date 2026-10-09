let i = 0; while (i < 3) { (() => { queue(() => i); i++; })(); }
