// Toast notifications, kept as state and rendered by the shell.

import { invalidate } from "../store.js";

/** type Toast = { id: Int, title: String, body: String, kind: String, until: Number } */

/** const toasts: Toast[] */
export const toasts = [];
let nextId = 1;

/** function toast(title: String, body: String, kind: String) => Undefined */
export function toast(title, body, kind) {
  const id = nextId;
  nextId = nextId + 1;
  toasts.push({ id: id, title: title, body: body, kind: kind, until: Date.now() + (kind === "error" ? 9000 : 4500) });
  if (toasts.length > 4) toasts.shift();
  invalidate();
  setTimeout(
    () => {
      expire();
    },
    kind === "error" ? 9100 : 4600,
  );
}

export function expire() {
  const now = Date.now();
  const keep = toasts.filter((t) => t.until > now);
  toasts.length = 0;
  for (const t of keep) toasts.push(t);
  invalidate();
}

/** function dismiss(id: Int) => Undefined */
export function dismiss(id) {
  const keep = toasts.filter((t) => t.id !== id);
  toasts.length = 0;
  for (const t of keep) toasts.push(t);
  invalidate();
}

/** function toastView(b: Builder) => Undefined */
export function toastView(b) {
  b.open("div", "toasts", "toasts");
  for (const t of toasts) {
    b.open("div", `t${t.id}`, `toast toast-${t.kind}`);
    b.on("click", (e) => dismiss(t.id));
    b.leaf("div", "title", "toast-title", t.title);
    if (t.body !== "") b.leaf("div", "body", "toast-body", t.body);
    b.close();
  }
  b.close();
}
