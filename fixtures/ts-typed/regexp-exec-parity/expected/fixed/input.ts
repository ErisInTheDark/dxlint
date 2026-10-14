/thing/.exec('something');
const search = /thing/;
search.exec('something');
/^[a-z]+thing\/?$/.exec('something');
'something'.match(/thing/g);
'something'.match('[');
function dynamic(pattern: string) { new RegExp(pattern).exec('check'); }
const pattern = 'thing';
RegExp(pattern).exec('something');
function shadow(RegExp: unknown) { 'something'.match(pattern); }
/\n/.exec('something');
'something'.match(('['));
