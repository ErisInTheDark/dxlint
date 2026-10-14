import { Promise } from "./lib";
async function fromValue() {
  return Promise.resolve(1);
}
async function fromEmpty() {
  return Promise.resolve();
}
