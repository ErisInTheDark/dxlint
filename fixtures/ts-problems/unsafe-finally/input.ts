function returned() { try {} finally { return 1; } }
function thrown() { try {} finally { throw error; } }
function outerBreak() { while (x) { try {} finally { break; } } }
function outerContinue() { for (;;) { try {} finally { continue; } } }
function outerSwitch() { switch (x) { default: try {} finally { break; } } }
function outerLabel() { out: { try {} finally { break out; } } }
function outerLabelContinue() { out: while (x) { try {} finally { continue out; } } }
function innerBreak() { try {} finally { while (x) { break; } } }
function innerContinue() { try {} finally { for (;;) { continue; } } }
function innerSwitch() { try {} finally { switch (x) { default: break; } } }
function innerLabel() { try {} finally { inside: { break inside; } } }
function innerLabelContinue() { try {} finally { inside: while (x) { continue inside; } } }
function nestedOwners() { try {} finally { function inner() { return 1; } const f = () => { throw error; }; class C { m() { return 2; } static { throw error; } } } }
function escaped() { try {} finally { \u006cabel: { break label; } } }
function nestedBreak() { try {} finally { while (x) { try {} finally { break; } } } }
function nestedReturn() { try {} finally { function inner() { try {} finally { return 2; } } } }
function nestedCatch() { try {} finally { try {} catch (error) { return 3; } } }
function safeTry() { try { return 1; } catch (error) { throw error; } finally { work(); } }
