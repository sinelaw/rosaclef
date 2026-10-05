// An in-memory backend for the UI tree: the "test renderer". It records the
// element tree and every primitive operation so tests can assert on the
// result of a view without a browser.

import { handle, handleIndex } from "#brands";

/** type MemNode = { type: String, cls: String, text: String, attrs: KS[], styles: KS[], props: KS[], children: Int[], parent: Int, alive: Boolean } */

/** type MemBackend = { backend: Backend, dump: () => String, ops: () => Int, fire: (String, String, Ev) => Boolean, nodes: () => MemNode[] } */

/** function blankEvent() => Ev */
export function blankEvent() {
  return {
    clientX: 0,
    clientY: 0,
    offsetX: 0,
    offsetY: 0,
    movementX: 0,
    movementY: 0,
    button: 0,
    buttons: 0,
    pointerId: 0,
    pointerType: "mouse",
    deltaX: 0,
    deltaY: 0,
    key: "",
    code: "",
    shiftKey: false,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    repeat: false,
    detail: 0,
    typing: false,
    terminal: false,
    onControl: false,
    value: "",
    checked: false,
    targetLeft: 0,
    targetTop: 0,
    targetWidth: 0,
    targetHeight: 0,
    scrollLeft: 0,
    scrollTop: 0,
    preventDefault: () => undefined,
    stopPropagation: () => undefined,
  };
}

/** function setKS(list: KS[], key: String, value: String) => Undefined */
function setKS(list, key, value) {
  const e = list.find((x) => x.key === key);
  if (e) e.value = value;
  else list.push({ key: key, value: value });
}

/** function memoryBackend() => MemBackend */
export function memoryBackend() {
  /** const nodes: MemNode[] */
  const nodes = [{ type: "root", cls: "", text: "", attrs: [], styles: [], props: [], children: [], parent: -1, alive: true }];
  /** const handlers: { handle: Handle, event: String, fn: (Ev) => Undefined }[] */
  const handlers = [];
  /** const frames: (() => Undefined)[] */
  const frames = [];
  let ops = 0;

  /** function node(h: Handle) => MemNode */
  function node(h) {
    return nodes[handleIndex(h)];
  }

  /** function detach(h: Handle) => Undefined */
  function detach(h) {
    const i = handleIndex(h);
    const p = nodes[i].parent;
    if (p >= 0) nodes[p].children = nodes[p].children.filter((c) => c !== i);
    nodes[i].parent = -1;
  }

  /** const backend: Backend */
  const backend = {
    create: (type) => {
      ops = ops + 1;
      nodes.push({ type: type, cls: "", text: "", attrs: [], styles: [], props: [], children: [], parent: -1, alive: true });
      return handle(nodes.length - 1);
    },
    root: () => handle(0),
    setText: (h, s) => {
      ops = ops + 1;
      node(h).text = s;
    },
    setClass: (h, c) => {
      ops = ops + 1;
      node(h).cls = c;
    },
    setAttr: (h, k, v) => {
      ops = ops + 1;
      setKS(node(h).attrs, k, v);
    },
    removeAttr: (h, k) => {
      ops = ops + 1;
      node(h).attrs = node(h).attrs.filter((a) => a.key !== k);
    },
    setStyle: (h, k, v) => {
      ops = ops + 1;
      if (v === "") node(h).styles = node(h).styles.filter((a) => a.key !== k);
      else setKS(node(h).styles, k, v);
    },
    setProp: (h, k, v) => {
      ops = ops + 1;
      setKS(node(h).props, k, v);
    },
    append: (p, c) => {
      ops = ops + 1;
      detach(c);
      node(p).children.push(handleIndex(c));
      node(c).parent = handleIndex(p);
    },
    remove: (h) => {
      ops = ops + 1;
      detach(h);
      node(h).alive = false;
    },
    listen: (h, event, fn) => {
      handlers.push({ handle: h, event: event, fn: fn });
    },
    paint: (h, fn) => undefined,
    frame: (fn) => {
      frames.push(fn);
    },
  };

  /** function dumpNode(h: Int, depth: Int) => String */
  function dumpNode(h, depth) {
    const n = nodes[h];
    let line = `${"  ".repeat(depth)}${n.type}`;
    if (n.cls !== "") line = `${line}.${n.cls.replaceAll(" ", ".")}`;
    if (n.text !== "") line = `${line} "${n.text}"`;
    for (const a of n.attrs) line = `${line} [${a.key}=${a.value}]`;
    let out = `${line}\n`;
    for (const c of n.children) out = out + dumpNode(c, depth + 1);
    return out;
  }

  return {
    backend: backend,
    dump: () => nodes[0].children.map((c) => dumpNode(c, 0)).join(""),
    ops: () => ops,
    /** Dispatch an event to the first live node with this class. */
    fire: (cls, event, e) => {
      for (const hd of handlers) {
        const n = node(hd.handle);
        if (n.alive && hd.event === event && n.cls.split(" ").includes(cls)) {
          hd.fn(e);
          return true;
        }
      }
      return false;
    },
    nodes: () => nodes,
  };
}
