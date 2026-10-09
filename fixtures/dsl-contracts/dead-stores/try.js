function f() { let a = 1; try { g(); a = 2; a = 3; } catch {} g(a); }
