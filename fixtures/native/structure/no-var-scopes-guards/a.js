var globalValue = 1;
{ var nestedGlobal = 1; }
function outside() { { var value = 1; } use(value); }
function caught() { try {} catch (e) { var e = 1; } }
function nestedCatch() { try {} catch (e) { { var e = 1; } } }
function nestedHandler() { try {} catch (e) { try {} catch (x) { var e = 1; } } }
function catchLoop() { try {} catch (e) { for (var e of []) {} } }
function early() { { use(value); var value = 1; } }
function self() { { var value = value; } }
function duplicate() { { var value = 1; var value = 2; } }
function retained() { for (let i of list) { var value; use(value); value = i; } }
function captured() { for (var value of list) { save(() => value); } }
function statement() { if (condition) var value = 1; }
function switchCase() { switch (condition) { case 0: var value = 1; } }
function dynamic() { { var value = 1; eval("value"); } }
function withScope() { with (object) { var value = 1; use(value); } }
