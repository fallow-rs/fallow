class Rect {
  constructor(width = 1, height = width ?? 1) {
    this.width = width;
    this.height = height;
  }

  get area() {
    if (this.width > 0) {
      return this.width * this.height;
    }
    return 0;
  }

  set area(value) {
    if (value > 0) {
      this.width = value / this.height;
      return;
    }
    this.width = 0;
  }
}

const rect = new Rect(4, 2);
console.log(rect.area);
