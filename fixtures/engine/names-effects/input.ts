x = 1;
x += 2;
y++;
[x, ...y] = [1, 2];
({ a: x, y } = o);
({ x = 3 } = o);
[(x)] = [1];
o.a = 2; delete o.b;
for (x of [1]) {}
