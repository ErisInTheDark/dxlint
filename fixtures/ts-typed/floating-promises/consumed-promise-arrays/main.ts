async function consume() {
const assigned = [Promise.resolve(1)];
void [Promise.resolve(1)];
await Promise.all(assigned);
Promise.all(assigned).catch(() => {});
return [Promise.resolve(1)];
}
