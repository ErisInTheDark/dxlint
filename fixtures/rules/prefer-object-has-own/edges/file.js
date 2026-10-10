// Unicode before byte spans: café
function a(){return{}.hasOwnProperty.call(obj, "x")}
async function b(){return await{}.hasOwnProperty.call(obj, "x")}
function* c(){yield{}.hasOwnProperty.call(obj, "x")}
for(const x of{}.hasOwnProperty.call(obj, "x").toString());
for(const x in{}.hasOwnProperty.call(obj, "x").toString());
function d(){return/*keep*/{}.hasOwnProperty.call(obj, /*key*/ "x")}
Object.prototype.hasOwnProperty.call /*gap*/ (obj, "x");
const nested = Object.prototype.hasOwnProperty.call(obj, Object.prototype.hasOwnProperty.call(obj, "x"));
