// Runtime side of the nominal index types (see web/types/brands.d.js):
// the type checker keeps InsertIx, TrackIx, ... apart; at runtime they are
// plain integers, so every cast is the identity.
const id = (n) => n;
export const insertIx = id;
export const insertIndex = id;
export const trackIx = id;
export const trackIndex = id;
export const noteIx = id;
export const noteIndex = id;
export const clipIx = id;
export const clipIndex = id;
export const laneIx = id;
export const laneIndex = id;
export const pointIx = id;
export const pointIndex = id;
