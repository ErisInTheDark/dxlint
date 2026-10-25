function a(Object) { return Object.prototype.hasOwnProperty.call(obj, "x"); }
function b(Object) { return ({}).hasOwnProperty.call(obj, "x"); }
obj.hasOwnProperty.call(obj, "x");
Object.prototype.other.call(obj, "x");
