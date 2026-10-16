function render(canvas: {width: number}, requiredCanvasWidth: number): void {
  if (canvas.width !== requiredCanvasWidth) {
    canvas.width = requiredCanvasWidth;
  }
}

function narrowUnit(x: 1 | 2): void {
  if (x !== 1) {
    const unit = x;
  }
}

function narrowWide(a: string, b: "x" | "y"): void {
  if (a !== b) {
    const wide = a;
  }
}
