import { describe, expect, it } from "vitest";
import { formatDataSize, limitSize, selectedLimit } from "./files-size";
import { fieldValue } from "./number-field";

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
  it("keeps the size to one decimal, as QDoubleSpinBox does", () => {
    // 2.25 is shown as 2.3; the limit stored and asked about is 2.3 GiB.
    expect(fieldValue(2.25, 0.1, 1024, 1)).toBe(2.3);
    expect(fieldValue(0.01, 0.1, 1024, 1)).toBe(0.1);
    expect(fieldValue(5000, 0.1, 1024, 1)).toBe(1024);
    expect(fieldValue(7.6, 1, 90)).toBe(8);
    expect(limitSize(0)).toBe(1);
    expect(limitSize(Math.round(2.25 * 1024 ** 3))).toBe(2.3);
    expect(selectedLimit("limit", limitSize(Math.round(0.1 * 1024 ** 3)))).toBe(Math.round(0.1 * 1024 ** 3));
  });
});
