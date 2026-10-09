class A { static[0] }
class B { ["x"] = 1 }
class C { ["constructor"]() {} }
class D { static ["prototype"]() {} }
class E { ["constructor"] = 1 }
const proto = { ["__proto__"]: value };
