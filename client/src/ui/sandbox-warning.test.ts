import { describe, expect, it } from "vitest";
import { sandboxWarningCopy } from "./sandbox-warning";

describe("the sandbox warning", () => {
  it("is Qt's blocking box, with the reason and what breaks", () => {
    const copy = sandboxWarningCopy("Started by another program's sandbox.");
    expect(copy.title).toBe("Zerus is in a sandbox");
    expect(copy.text).toContain("Started by another program's sandbox.");
    expect(copy.text).toContain('settings, folders and the log may be refused with "access denied"');
    expect(copy.quit).toBe("Quit Zerus");
    expect(copy.stay).toBe("Continue anyway");
  });
});
