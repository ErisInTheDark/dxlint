x = y * x;
obj[key] = obj[key] + y;
obj.child.value = obj.child.value + y;
x = /* keep */ x + y;
obj.a = obj?.a + y;
(obj?.a).b = (obj?.a).b + y;
