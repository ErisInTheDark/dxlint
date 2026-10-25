var object = { get bar() {} };
object = { get
 bar () {} };

class GetterClass { get bar() {} }

var descriptor = Object.defineProperty(object, "bar", { get: function () {} });
descriptor = Object.defineProperty(object, "bar", { get: () => {} });
descriptor = Object.create(object, { bar: { get: function () {} } });

descriptor = Reflect.defineProperty(object, "bar", { get() {} });
descriptor = Object.defineProperties(object, { bar: { get() {} } });

var ok = { get bar() { return true; } };
