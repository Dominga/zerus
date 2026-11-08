import { describe, expect, it } from "vitest";
import { hintedMicrophone } from "./voice-hint";

// Task 102: Qt's microphone arrives as its name; the listed device with that
// name is chosen once, and a raw Qt device id is never stored.
describe("the imported microphone name", () => {
  const listed = [
    { id: "{0.0.1.00000000}.{cpal-a}", name: "Microphone (Realtek(R) Audio)" },
    { id: "{0.0.1.00000000}.{cpal-b}", name: "Headset (Bluetooth)" },
  ];
  it("chooses the listed device with that name and drops the hint", () => {
    expect(hintedMicrophone(listed, "", "Microphone (Realtek(R) Audio)")).toEqual({ choose: "{0.0.1.00000000}.{cpal-a}", drop: true });
    expect(hintedMicrophone(listed, "", "  Headset (Bluetooth) ")).toEqual({ choose: "{0.0.1.00000000}.{cpal-b}", drop: true });
  });
  it("waits for a device that is not connected, and never overrides a chosen one", () => {
    expect(hintedMicrophone(listed, "", "USB Mic")).toEqual({ drop: false });
    expect(hintedMicrophone([], "", "Microphone (Realtek(R) Audio)")).toEqual({ drop: false });
    expect(hintedMicrophone(listed, "{0.0.1.00000000}.{cpal-b}", "Microphone (Realtek(R) Audio)")).toEqual({ drop: true });
    expect(hintedMicrophone(listed, "", "")).toEqual({ drop: false });
  });
  it("never chooses by a Qt device id", () => {
    // Qt's id is not a name: no listed device is named like it.
    expect(hintedMicrophone(listed, "", "{0.0.1.00000000}.{1b15d461-3242-454f-b577-749d304e90b4}")).toEqual({ drop: false });
  });
});
