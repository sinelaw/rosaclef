// Nominal index types (loaded with `inty --lib` before globals.d.js).
//
// Integers with different meanings must not be mixed up: a mixer insert is
// not a playlist track is not a note. inty treats each class as its own
// nominal type; at runtime they are plain integers, created and unwrapped
// with the identity casts exported by "#brands".

/** Index into mixer.inserts (0 = master). */
class InsertIx {}
/** Index into playlist.tracks. */
class TrackIx {}
/** Index into a pattern's notes. */
class NoteIx {}
/** Index into playlist.clips. */
class ClipIx {}
/** A UI backend element handle. */
class Handle {}
/** Index into a description buffer. */
class NodeIx {}
