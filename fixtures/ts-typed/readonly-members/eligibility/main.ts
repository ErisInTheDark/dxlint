namespace Case414 {

class TestProtectedModifiable {
  protected protectedModifiable = 7;
}
    
}
namespace Case419 {

class TestPublicModifiable {
  public publicModifiable = 7;
}
    
}
namespace Case93 {

class TestReadonlyInline {
  private readonly correctlyReadonlyInline = 7;
}
    
}
namespace Case727 {

class TestPrivateAccessor {
  private accessor acc = 3;
}
    
}
namespace Case732 {

class TestPrivateFieldAccessor {
  accessor #acc = 3;
}
    
}
namespace Case804 {

class TestIncorrectlyModifiableStatic {
  private static incorrectlyModifiableStatic = 7;
}
      
}
