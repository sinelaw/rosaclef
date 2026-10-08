// Previews: who has the engine playing a version of the song of their own
// (the Drums tab's preview or a groove tried out, the Mix check's audition).
// While one does, project changes go to its `onChange` instead of the engine
// (store.js asks `previewTakes`), and the next one to take the engine stops it
// first. No imports: the ownership rules are tested on their own
// (test/preview.test.js).

/** const preview: { owner: String, onChange: () => Undefined, stop: () => Undefined } */
const preview = { owner: "", onChange: () => undefined, stop: () => undefined };

/** Take the engine for a preview. `onChange` runs instead of giving the engine
 * each change to the song; `stop` ends the preview if another one takes the
 * engine (the previous owner's `stop` runs first). */
/** function claimPreview(owner: String, onChange: () => Undefined, stop: () => Undefined) => Undefined */
export function claimPreview(owner, onChange, stop) {
  if (preview.owner !== "" && preview.owner !== owner) {
    const end = preview.stop;
    // Released first, so its own release (in `stop`) does nothing.
    preview.owner = "";
    end();
  }
  preview.owner = owner;
  preview.onChange = onChange;
  preview.stop = stop;
}

/** Give the engine back to the song, if `owner` still has it (another
 * preview may have taken it meanwhile). */
/** function releasePreview(owner: String) => Undefined */
export function releasePreview(owner) {
  if (preview.owner !== owner) return undefined;
  preview.owner = "";
  preview.onChange = () => undefined;
  preview.stop = () => undefined;
}

/** Who has the engine ("" = the song). */
/** function previewOwner() => String */
export function previewOwner() {
  return preview.owner;
}

/** The song changed: a preview playing takes the change (true), or the
 * engine should get it (false). */
/** function previewTakes() => Boolean */
export function previewTakes() {
  if (preview.owner === "") return false;
  preview.onChange();
  return true;
}
