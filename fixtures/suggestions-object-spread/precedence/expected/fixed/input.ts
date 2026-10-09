const a = { ...(test ? foo : bar), ...(x => x), ...(foo = bar)};
foo()
;({ ...bar}).value;
if (test) {}
({ ...bar});
({ ...foo})();
foo()
;({ ...foo})();
call({ ...foo});
