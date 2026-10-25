const α = <A>
  <B />
</A>;
const item =
  <A>
    <B />
  </A>;
const object = {
  value:
    // keep this
    <A>
      <B />
    </A>
};
const prop = <A value={
  <B>
    <C />
  </B>
} />;
