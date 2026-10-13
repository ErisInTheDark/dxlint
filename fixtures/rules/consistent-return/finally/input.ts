declare function cleanup(): void;
declare function load(): Promise<number>;

function direct() {
  try { return 1; } finally { cleanup(); }
}

async function asynchronous() {
  try { return await load(); } finally { cleanup(); }
}

function caught() {
  try { return 1; } catch { return 2; } finally { cleanup(); }
}

function nested() {
  try { try { return 1; } finally { cleanup(); } } finally { cleanup(); }
}

function conditional(flag: boolean) {
  try { if (flag) return 1; } finally { cleanup(); }
}
