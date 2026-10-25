const ignored = () => <A>
  <B />
</A>;
const object = { value: <A>
  <B />
</A> };
const single = <A />;
const keep =
  <A>
    <B />
  </A> /* never delete */;
