// Types of the tree UI library (loaded with `inty --lib`, before the host's
// own declarations, which may use them: Ev, Ctx, Builder, ...).

// ------------------------------------------------------------------ newtypes
// Construct and unwrap them with the casts exported by "#tree-ids" (./ids.js).

/** A UI backend element handle. */
/** nominal type Handle = Int */
/** Index into a description buffer. */
/** nominal type NodeIx = Int */

/** A key/value pair of strings (attributes, styles, properties). */
/** type KS = { key: String, value: String } */

// ------------------------------------------------------------------ events
// What a listener receives (./dom.js enriches the browser's event with the
// target's box and scroll, a field's value and whether the user is typing).

/** type Ev = {
    clientX: Number, clientY: Number, offsetX: Number, offsetY: Number,
    movementX: Number, movementY: Number, button: Number, buttons: Number, pointerId: Number, pointerType: String,
    deltaX: Number, deltaY: Number, key: String, code: String,
    shiftKey: Boolean, ctrlKey: Boolean, metaKey: Boolean, altKey: Boolean, repeat: Boolean,
    detail: Number, typing: Boolean, terminal: Boolean, onControl: Boolean, value: String, checked: Boolean,
    targetLeft: Number, targetTop: Number, targetWidth: Number, targetHeight: Number,
    scrollLeft: Number, scrollTop: Number,
    preventDefault: () => Undefined, stopPropagation: () => Undefined
} */

// ------------------------------------------------------------------ canvas

/** type Gradient = { addColorStop: (Number, String) => Undefined } */

/** type Ctx = {
    fillStyle: String, strokeStyle: String, lineWidth: Number, font: String,
    textAlign: String, textBaseline: String, globalAlpha: Number,
    shadowColor: String, shadowBlur: Number, lineCap: String, lineJoin: String,
    fillRect: (Number, Number, Number, Number) => Undefined,
    strokeRect: (Number, Number, Number, Number) => Undefined,
    clearRect: (Number, Number, Number, Number) => Undefined,
    beginPath: () => Undefined, closePath: () => Undefined,
    moveTo: (Number, Number) => Undefined, lineTo: (Number, Number) => Undefined,
    rect: (Number, Number, Number, Number) => Undefined,
    roundRect: (Number, Number, Number, Number, Number) => Undefined,
    arc: (Number, Number, Number, Number, Number) => Undefined,
    quadraticCurveTo: (Number, Number, Number, Number) => Undefined,
    fill: () => Undefined, stroke: () => Undefined, clip: () => Undefined,
    fillText: (String, Number, Number) => Undefined,
    measureText: (String) => { width: Number },
    save: () => Undefined, restore: () => Undefined,
    translate: (Number, Number) => Undefined, scale: (Number, Number) => Undefined,
    setLineDash: (Number[]) => Undefined,
    createLinearGradient: (Number, Number, Number, Number) => Gradient,
    fillGradient: (Gradient) => Undefined,
    strokeGradient: (Gradient) => Undefined
} */

// ------------------------------------------------------------- descriptions
// See ./tree.js. Descriptions are flat (parent index, not nested children),
// which keeps them cheap to rebuild and easy to type.

/** type Listener = { event: String, fn: (Ev) => Undefined } */
/** type Painter = (Ctx, Number, Number) => Undefined */
/** type Desc = { parent: NodeIx, type: String, key: String, cls: String, text: String, attrs: KS[], styles: KS[], props: KS[], on: Listener[], paint: Painter, canvas: Boolean } */

/** type Builder = {
    open: (String, String, String) => Undefined,
    close: () => Undefined,
    leaf: (String, String, String, String) => Undefined,
    text: (String) => Undefined,
    attr: (String, String) => Undefined,
    style: (String, String) => Undefined,
    prop: (String, String) => Undefined,
    on: (String, (Ev) => Undefined) => Undefined,
    canvas: (String, String, Painter) => Undefined,
    nodes: () => Desc[]
} */

/** type Backend = {
    create: (String) => Handle,
    root: () => Handle,
    setText: (Handle, String) => Undefined,
    setClass: (Handle, String) => Undefined,
    setAttr: (Handle, String, String) => Undefined,
    removeAttr: (Handle, String) => Undefined,
    setStyle: (Handle, String, String) => Undefined,
    setProp: (Handle, String, String) => Undefined,
    append: (Handle, Handle) => Undefined,
    insert: (Handle, Handle, Handle) => Undefined,
    remove: (Handle) => Undefined,
    listen: (Handle, String, (Ev) => Undefined) => Undefined,
    paint: (Handle, Painter) => Undefined,
    frame: (() => Undefined) => Undefined
} */
