// Unicode before byte spans: café
function a(){return Object.hasOwn(obj, "x")}
async function b(){return await Object.hasOwn(obj, "x")}
function* c(){yield Object.hasOwn(obj, "x")}
for(const x of Object.hasOwn(obj, "x").toString());
for(const x in Object.hasOwn(obj, "x").toString());
function d(){return/*keep*/Object.hasOwn(obj, /*key*/ "x")}
Object.hasOwn /*gap*/ (obj, "x");
const nested = Object.hasOwn(obj, Object.hasOwn(obj, "x"));
