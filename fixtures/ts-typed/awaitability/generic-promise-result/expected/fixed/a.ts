import { Promise } from "./lib";
async function fromValue() {
  return await Promise.resolve(1);
}
async function fromEmpty() {
  return await Promise.resolve();
}
