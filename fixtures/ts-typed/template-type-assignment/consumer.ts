import type { Id } from "./input";
let imported: Id;
imported = `5,6`;
class ImportedHolder {
  islandId: Id;
  update() {
    this.islandId = `7,8`;
  }
}
