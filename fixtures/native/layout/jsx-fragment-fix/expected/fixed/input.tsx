const nested = <><>{/*keep*/}雪🙂</></>;
const siblings = <><><div /></><><span /></></>;
const selfClosed = <Fragment />;
const generic = <Fragment<T>>x</Fragment>;
const openingComment = <Fragment /*keep*/>x</Fragment>;
const closingComment = <Fragment>x</Fragment /*keep*/>;
const attributes = <Fragment key="keep">x</Fragment>;
const other = <Other.Fragment>x</Other.Fragment>;
