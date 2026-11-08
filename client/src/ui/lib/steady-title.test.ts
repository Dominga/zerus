import { describe, expect, it } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { steadyTitle } from "./steady-title";

describe("a title under the pointer (zerus-5ryrx)", () => {
  it("follows its text until the pointer enters, holds while it stays and follows again once it leaves", () => {
    createRoot((dispose) => {
      const [text, setText] = createSignal("Working (9m)");
      const tip = steadyTitle(text);
      expect(tip.title()).toBe("Working (9m)");
      setText("Working (10m)");
      expect(tip.title(), "unhovered, it updates").toBe("Working (10m)");
      tip.enter();
      for (const next of ["Working (11m)", "Working (12m)", "Ready"]) {
        setText(next);
        expect(tip.title(), "hovered, it holds").toBe("Working (10m)");
      }
      tip.leave();
      expect(tip.title()).toBe("Ready");
      tip.enter();
      expect(tip.title(), "a new hover reads the text again").toBe("Ready");
      dispose();
    });
  });
});
