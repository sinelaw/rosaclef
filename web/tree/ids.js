// Runtime side of the tree library's nominal types (see ./ids.d.js): the type
// checker keeps element handles and description indexes apart from plain
// integers; at runtime every cast is the identity.
const id = (n) => n;
export const handle = id;
export const handleIndex = id;
export const nodeIx = id;
export const nodeIndex = id;
