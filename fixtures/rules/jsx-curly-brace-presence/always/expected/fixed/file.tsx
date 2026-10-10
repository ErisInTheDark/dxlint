const α = <A value={"foo \"bar\""} other={"raw \\n"}>{"child \"quote\" \\u1234"}</A>;
const β = <A>
  {"first"}
    {"second   "}
  &lt;{"Component"}&gt;
  &nbsp;<B />&nbsp;
</A>;
