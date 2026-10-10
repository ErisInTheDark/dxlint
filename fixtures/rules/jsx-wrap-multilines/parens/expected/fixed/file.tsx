const α = (<A>
  <B />
</A>);
const β = () => (<>
  <B />
</>);
const choice = flag ? (<A>
  <B />
</A>) : (<C>
  <D />
</C>);
const nested = (<Outer prop={(<Inner>
  <B />
</Inner>)}>
  <C />
</Outer>);
function render() {
  return (<A>
    <B />
  </A>);
}
