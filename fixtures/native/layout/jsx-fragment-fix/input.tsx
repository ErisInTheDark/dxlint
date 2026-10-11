const nested = <Fragment><React.Fragment>{/*keep*/}雪🙂</React.Fragment></Fragment>;
const siblings = <><Fragment><div /></Fragment><React.Fragment><span /></React.Fragment></>;
const selfClosed = <Fragment />;
const generic = <Fragment<T>>x</Fragment>;
const openingComment = <Fragment /*keep*/>x</Fragment>;
const closingComment = <Fragment>x</Fragment /*keep*/>;
const attributes = <Fragment key="keep">x</Fragment>;
const other = <Other.Fragment>x</Other.Fragment>;
