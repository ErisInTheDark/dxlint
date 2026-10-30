// JavaScript has no accessibility modifiers, so nothing here can be reported.
class Window {
  width = 1;
  show() { return this.width; }
  get size() { return this.width; }
}
module.exports = Window;
