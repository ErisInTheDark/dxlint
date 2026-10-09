namespace Case204 {

class TestModifiableWithinConstructorArrowFunction {
  private correctlyModifiableWithinConstructorArrowFunction = 7;

  public constructor() {
    (() => {
      this.correctlyModifiableWithinConstructorArrowFunction += 1;
    })();
  }
}
    
}
namespace Case215 {

class TestModifiableWithinConstructorArrowFunction {
  #correctlyModifiableWithinConstructorArrowFunction = 7;

  public constructor() {
    (() => {
      this.#correctlyModifiableWithinConstructorArrowFunction += 1;
    })();
  }
}
    
}
namespace Case226 {

class TestModifiableWithinConstructorInFunctionExpression {
  private correctlyModifiableWithinConstructorInFunctionExpression = 7;

  public constructor() {
    const self = this;

    (() => {
      self.correctlyModifiableWithinConstructorInFunctionExpression += 1;
    })();
  }
}
    
}
namespace Case252 {

class TestModifiableWithinConstructorInGetAccessor {
  private correctlyModifiableWithinConstructorInGetAccessor = 7;

  public constructor() {
    const self = this;

    const confusingObject = {
      get accessor() {
        return (self.correctlyModifiableWithinConstructorInGetAccessor += 1);
      },
    };
  }
}
    
}
namespace Case282 {

class TestModifiableWithinConstructorInMethodDeclaration {
  private correctlyModifiableWithinConstructorInMethodDeclaration = 7;

  public constructor() {
    const self = this;

    const confusingObject = {
      methodDeclaration() {
        self.correctlyModifiableWithinConstructorInMethodDeclaration = 7;
      },
    };
  }
}
    
}
