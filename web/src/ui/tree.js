// A small retained, reconciling UI tree — in the spirit of fresh-ui
// (https://github.com/sinelaw/fresh/tree/master/crates/fresh-ui).
//
// Three layers, in order of cost:
//
//  1. Descriptions (`Desc`) — plain values rebuilt from state on every
//     flush. They have no identity and no side effects. They are *flat*: each
//     node names its parent's index, so building one costs one allocation.
//  2. Elements — persistent records matched to descriptions by their path of
//     (type, key) pairs. They own the backend handle, the last applied
//     properties and the current event handlers, and they survive rebuilds.
//  3. The backend — a set of primitive operations on numeric handles (the
//     DOM in the browser, an in-memory tree in tests). Nothing else touches
//     the platform.
//
// State flows down (a view is a function of state writing descriptions),
// events flow up (handlers are callbacks in the description). There are no
// signals and no observers: anything that changes calls `mark()`, and the
// next frame rebuilds and reconciles everything.

import { nodeIx, nodeIndex } from "#brands";

/** function noPaint(g: Ctx, w: Number, h: Number) => Undefined */
function noPaint(g, w, h) {
  return undefined;
}

/** The description builder: a stack of open nodes over a flat buffer. */
/** function builder() => Builder */
export function builder() {
  /** const nodes: Desc[] */
  const nodes = [];
  /** const stack: NodeIx[] */
  const stack = [];
  let current = -1;

  /** function add(type: String, key: String, cls: String) => Undefined */
  function add(type, key, cls) {
    const parent = stack.length > 0 ? stack[stack.length - 1] : nodeIx(-1);
    nodes.push({ parent: parent, type: type, key: key, cls: cls, text: "", attrs: [], styles: [], props: [], on: [], paint: noPaint, canvas: false });
    current = nodes.length - 1;
    return undefined;
  }

  return {
    open: (type, key, cls) => {
      add(type, key, cls);
      stack.push(nodeIx(current));
      return undefined;
    },
    close: () => {
      // Modifiers after close() apply to the node just closed.
      const closed = stack.pop();
      if (closed !== undefined) current = nodeIndex(closed);
      return undefined;
    },
    leaf: (type, key, cls, text) => {
      add(type, key, cls);
      nodes[current].text = text;
      return undefined;
    },
    text: (s) => {
      if (current >= 0) nodes[current].text = s;
      return undefined;
    },
    attr: (k, v) => {
      if (current >= 0) nodes[current].attrs.push({ key: k, value: v });
      return undefined;
    },
    style: (k, v) => {
      if (current >= 0) nodes[current].styles.push({ key: k, value: v });
      return undefined;
    },
    prop: (k, v) => {
      if (current >= 0) nodes[current].props.push({ key: k, value: v });
      return undefined;
    },
    on: (event, fn) => {
      if (current >= 0) nodes[current].on.push({ event: event, fn: fn });
      return undefined;
    },
    canvas: (key, cls, paint) => {
      add("canvas", key, cls);
      nodes[current].paint = paint;
      nodes[current].canvas = true;
      return undefined;
    },
    nodes: () => nodes,
  };
}

// ------------------------------------------------------------ reconciliation

/** type Elem = { path: String, handle: Handle, type: String, cls: String, text: String, attrs: KS[], styles: KS[], props: KS[], on: Listener[], bound: String[], order: String[], seen: Int } */

/** function sameKS(a: KS[], b: KS[]) => Boolean */
function sameKS(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i].key !== b[i].key || a[i].value !== b[i].value) return false;
  }
  return true;
}

/** function findKS(list: KS[], key: String) => String? */
function findKS(list, key) {
  const e = list.find((x) => x.key === key);
  return e ? e.value : undefined;
}

/** type Ui = { mark: () => Undefined, flush: () => Undefined, stats: () => { elements: Int, flushes: Int, created: Int } } */

/** Mount a view onto a backend. */
/** function mount(backend: Backend, view: (Builder) => Undefined) => Ui */
export function mount(backend, view) {
  const elems = new Map();
  let generation = 0;
  let flushes = 0;
  let created = 0;
  let queued = false;
  const rootPath = "";
  /** const rootElem: Elem */
  const rootElem = { path: rootPath, handle: backend.root(), type: "root", cls: "", text: "", attrs: [], styles: [], props: [], on: [], bound: [], order: [], seen: 0 };

  /** function update(el: Elem, d: Desc) => Undefined */
  function update(el, d) {
    const h = el.handle;
    if (el.cls !== d.cls) {
      backend.setClass(h, d.cls);
      el.cls = d.cls;
    }
    if (el.text !== d.text) {
      backend.setText(h, d.text);
      el.text = d.text;
    }
    if (!sameKS(el.attrs, d.attrs)) {
      for (const a of el.attrs) {
        if (findKS(d.attrs, a.key) === undefined) backend.removeAttr(h, a.key);
      }
      for (const a of d.attrs) {
        if (findKS(el.attrs, a.key) !== a.value) backend.setAttr(h, a.key, a.value);
      }
      el.attrs = d.attrs;
    }
    if (!sameKS(el.styles, d.styles)) {
      for (const s of el.styles) {
        if (findKS(d.styles, s.key) === undefined) backend.setStyle(h, s.key, "");
      }
      for (const s of d.styles) {
        if (findKS(el.styles, s.key) !== s.value) backend.setStyle(h, s.key, s.value);
      }
      el.styles = d.styles;
    }
    // Handlers: bind one backend listener per event type; it dispatches to
    // whatever handler the latest description holds.
    el.on = d.on;
    for (const l of d.on) {
      if (!el.bound.includes(l.event)) {
        el.bound.push(l.event);
        const ev = l.event;
        backend.listen(h, ev, (e) => {
          const cur = el.on.find((x) => x.event === ev);
          if (cur) cur.fn(e);
          return undefined;
        });
      }
    }
    return undefined;
  }

  /** DOM properties go last: a select's value needs its options. */
  /** function updateProps(el: Elem, d: Desc) => Undefined */
  function updateProps(el, d) {
    if (!sameKS(el.props, d.props)) {
      for (const p of d.props) {
        if (findKS(el.props, p.key) !== p.value) backend.setProp(el.handle, p.key, p.value);
      }
      el.props = d.props;
    }
    return undefined;
  }

  function flush() {
    queued = false;
    flushes = flushes + 1;
    generation = generation + 1;
    const b = builder();
    view(b);
    const nodes = b.nodes();
    // Children of each node, root first (index -1 stored at the end).
    /** const kids: Int[][] */
    const kids = nodes.map((_) => []);
    /** const rootKids: Int[] */
    const rootKids = [];
    for (let i = 0; i < nodes.length; i++) {
      const p = nodeIndex(nodes[i].parent);
      if (p < 0) rootKids.push(i);
      else kids[p].push(i);
    }
    /** const paths: String[] */
    const paths = nodes.map((_) => "");
    /** const painters: Int[] */
    const painters = [];

    /** function place(parent: Elem, children: Int[]) => Undefined */
    function place(parent, children) {
      /** const order: String[] */
      const order = [];
      const counts = new Map();
      for (const i of children) {
        const d = nodes[i];
        let key = d.key;
        if (key === "") {
          const n = counts.get(d.type) ?? 0;
          counts.set(d.type, n + 1);
          key = `@${n}`;
        }
        const path = `${parent.path}/${d.type}:${key}`;
        paths[i] = path;
        let el = elems.get(path);
        if (el === undefined || el.seen === generation) {
          const handle = backend.create(d.type);
          created = created + 1;
          el = { path: path, handle: handle, type: d.type, cls: "", text: "", attrs: [], styles: [], props: [], on: [], bound: [], order: [], seen: 0 };
          elems.set(path, el);
        }
        el.seen = generation;
        update(el, d);
        order.push(path);
        if (d.canvas) painters.push(i);
        place(el, kids[i]);
        updateProps(el, d);
      }
      // Re-attach children only when their order changed.
      let same = order.length === parent.order.length;
      if (same) {
        for (let i = 0; i < order.length; i++) {
          if (order[i] !== parent.order[i]) {
            same = false;
            break;
          }
        }
      }
      if (!same) {
        for (const path of order) {
          const child = elems.get(path);
          if (child) backend.append(parent.handle, child.handle);
        }
        parent.order = order;
      }
      return undefined;
    }

    rootElem.seen = generation;
    place(rootElem, rootKids);

    // Dispose elements that were not described this time.
    /** const gone: String[] */
    const gone = [];
    elems.forEach((el, path) => {
      if (el.seen !== generation) gone.push(path);
      return undefined;
    });
    for (const path of gone) {
      const el = elems.get(path);
      if (el) {
        backend.remove(el.handle);
        elems.delete(path);
      }
    }

    for (const i of painters) {
      const el = elems.get(paths[i]);
      if (el) backend.paint(el.handle, nodes[i].paint);
    }
    return undefined;
  }

  return {
    mark: () => {
      if (!queued) {
        queued = true;
        backend.frame(flush);
      }
      return undefined;
    },
    flush: flush,
    stats: () => ({ elements: elems.size, flushes: flushes, created: created }),
  };
}
