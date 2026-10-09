a==b;
a == null;
true == 1;
'wee' == /wee/;
`${value}` == 'value';
typeof a == b;
b != typeof a;
typeof a == ({ valueOf() { return 'number'; } });
/a/ == /a/;
