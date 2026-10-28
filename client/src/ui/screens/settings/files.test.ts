import { describe, expect, it } from "vitest";
import { formatDataSize, selectedLimit } from "./files-size";

describe("Settings > Files", () => {
  it("writes sizes as QLocale::formattedDataSize does", () => {
    expect(formatDataSize(0)).toBe("0 bytes");
    expect(formatDataSize(1023)).toBe("1023 bytes");
    expect(formatDataSize(1024)).toBe("1.00 KiB");
    expect(formatDataSize(1536 * 1024)).toBe("1.50 MiB");
    expect(formatDataSize(1024 ** 3)).toBe("1.00 GiB");
    expect(formatDataSize(5 * 1024 ** 4)).toBe("5.00 TiB");
  });
  it("chooses no limit for Never clear and whole bytes otherwise", () => {
    expect(selectedLimit("never", 3)).toBe(0);
    expect(selectedLimit("limit", 1)).toBe(1024 ** 3);
    expect(selectedLimit("limit", 0.1)).toBe(Math.round(0.1 * 1024 ** 3));
  });
});
