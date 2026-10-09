import Store, { type Box } from "@app/a";
class Holder {
  store = new Store();
  run(box: Box) {
    this.store.save();
    box.store.value;
    this.store.stop();
    this.store.on(1);
  }
}
