try {
  work();
}
 catch (e) {
  recover();
}
 finally {
  done();
}
