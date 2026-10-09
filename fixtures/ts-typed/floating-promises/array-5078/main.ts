
const data = ['test'];
data.map(async () => {
  await new Promise((_res, rej) => setTimeout(rej, 1000));
});
      