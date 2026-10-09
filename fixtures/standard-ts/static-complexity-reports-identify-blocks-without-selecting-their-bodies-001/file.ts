class C { field = x || y; static { if (x) y(); } static /* keep */
{ if (x) { y(); y(); y(); } } }