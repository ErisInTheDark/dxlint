function normal() {
  for (const key in arguments) {}
  let alias = arguments;
  for (const key in alias) {}
  const arrow = () => { for (const key in arguments) {} };
}
class C { method() { for (const key in arguments) {} } }
function shadow(arguments: { plain: number }) {
  for (const key in arguments) {}
}
for (const key in arguments) {}
