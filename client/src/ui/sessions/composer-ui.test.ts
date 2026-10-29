import { describe, expect, it } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { keptItem, moveFocusOff } from "./composer-ui";
import type { AttachmentView } from "../../types";

const file = (reference: string, name: string): AttachmentView => ({ name, mime: "text/plain", reference, bytes: 10, path: `draft/1/1/${reference}`, excerpt: null });

describe("keptItem", () => {
  it("a row reads its own attachment by reference, also in a new list", () => {
    createRoot((dispose) => {
      const [items, setItems] = createSignal([file("[File #1]", "a.txt"), file("[File #2]", "b.txt")]);
      const second = keptItem(items, "[File #2]");
      expect(second().name).toBe("b.txt");
      // A new view: new objects, another order, a renamed path.
      setItems([{ ...file("[File #2]", "b.txt"), path: "draft/1/2/0" }, file("[File #1]", "a.txt")]);
      expect(second().path).toBe("draft/1/2/0");
      dispose();
    });
  });

  // Fix round 2: a row whose reference went may read once more before Solid disposes it.
  it("a row whose reference went keeps its own last item, never another row's", () => {
    createRoot((dispose) => {
      const [items, setItems] = createSignal([file("[File #1]", "a.txt"), file("[File #2]", "b.txt")]);
      const second = keptItem(items, "[File #2]");
      setItems([file("[File #1]", "a.txt")]);
      expect(second().name).toBe("b.txt");
      setItems([]);
      expect(second().name).toBe("b.txt");
      dispose();
    });
  });
});

describe("moveFocusOff", () => {
  const field = (extra: { disabled?: boolean; isConnected?: boolean } = {}) => {
    const target = { focused: 0, focus() { target.focused++; }, ...extra };
    return target;
  };

  it("the transcript takes the focus when it can; else the message field (Qt's toolbar focus fallback)", () => {
    const shown = field();
    moveFocusOff(() => true, shown);
    expect(shown.focused).toBe(0);
    moveFocusOff(() => false, shown);
    expect(shown.focused).toBe(1);
  });

  it("a disabled or detached field does not take it, and none is no error", () => {
    const disabled = field({ disabled: true }), detached = field({ isConnected: false });
    moveFocusOff(() => false, disabled);
    moveFocusOff(() => false, detached);
    moveFocusOff(() => false, undefined);
    expect(disabled.focused + detached.focused).toBe(0);
  });
});
