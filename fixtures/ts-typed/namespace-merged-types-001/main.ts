import { IslandId, Box, Shape, Direction, Types } from "./types";
import type { IslandId as TypeIslandId } from "./types";

function inspect(
  id: IslandId,
  typeId: TypeIslandId,
  box: Box,
  shape: Shape,
  direction: Direction,
  nested: Box.Nested,
  entry: Types.Entry,
  queried: typeof IslandId,
): void {
  const checkStringAlias = id;
  const checkStringTypeImport = typeId;
  const checkObjectClass = box;
  const checkNumberClass = box.value;
  const checkStructuralInterface = shape;
  const checkNumberInterface = shape.value;
  const checkNumberEnum = direction;
  const checkBooleanNested = nested;
  const checkStringQualified = entry;
  const checkNumberValue = IslandId.marker;
  const checkNumberQueried = queried.marker;
  const checkNumberClassValue = Box.marker;
  const checkNumberInterfaceValue = Shape.marker;
  const checkNumberEnumValue = Direction.marker;
}
