'something'.match(/thing/);
const search = /thing/;
'something'.match(search);
'something'.match('^[a-z]+thing/?$');
'something'.match(/thing/g);
'something'.match('[');
function dynamic(pattern: string) { 'check'.match(new RegExp(pattern)); }
const pattern = 'thing';
'something'.match(pattern);
function shadow(RegExp: unknown) { 'something'.match(pattern); }
'something'.match('\\\n');
'something'.match(('['));
