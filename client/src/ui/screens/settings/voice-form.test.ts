import { describe, expect, it } from "vitest";
import { endpointValid, formValid, type AdapterView, type EditorForm } from "./voice-form";

// The same cases as client/core/src/speech_settings/tests.rs, whose rules
// these mirror.

const adapters: AdapterView[] = [
  { id: "gigaam", label: "Local GigaAM", models: ["v3_e2e_rnnt"], devices: ["cpu"] },
  { id: "openai_http", label: "OpenAI-compatible server", models: [], devices: [] },
];
const remote: EditorForm = {
  name: "Remote fixture", id: "remote-fixture", adapter: "openai_http", model: "gigaam", language: "ru", device: "",
  base_url: "https://speech.example.net/v1", allow_insecure_http: false, credential: "replace", key: "synthetic-private-key",
};
const local: EditorForm = { ...remote, adapter: "gigaam", model: "v3_e2e_rnnt", device: "cpu", base_url: "", credential: "keep", key: "" };
const valid = (change: Partial<EditorForm>, base = remote) => formValid({ ...base, ...change }, adapters);

describe("the provider editor's form", () => {
  it("accepts what Qt accepts", () => {
    expect(valid({})).toBe(true);
    expect(valid({}, local)).toBe(true);
    expect(valid({ base_url: "http://127.0.0.1:9000/v1", allow_insecure_http: true })).toBe(true);
    expect(valid({ key: "", credential: "keep" })).toBe(true);
    expect(valid({ language: "" })).toBe(true);
  });
  it("refuses what Qt refuses", () => {
    expect(valid({ base_url: "http://127.0.0.1:9000/v1" })).toBe(false);
    expect(valid({ base_url: "https://user:secret@example.net/v1" })).toBe(false);
    expect(valid({ base_url: "https://speech.example.net/v1?key=secret" })).toBe(false);
    expect(valid({ base_url: "https://speech.example.net/v1#part" })).toBe(false);
    expect(valid({ base_url: "https:///v1" })).toBe(false);
    expect(valid({ base_url: "https://speech example.net/v1" })).toBe(false);
    expect(valid({ key: "" })).toBe(false);
    expect(valid({ credential: "forget" })).toBe(false);
    expect(valid({ id: "has space" })).toBe(false);
    expect(valid({ id: "x".repeat(65) })).toBe(false);
    expect(valid({ name: "   " })).toBe(false);
    expect(valid({ language: "RU" })).toBe(false);
    expect(valid({ model: "unknown" }, local)).toBe(false);
    expect(valid({ device: "mps" }, local)).toBe(false);
  });
  it("reads endpoints as the core does", () => {
    expect(endpointValid("https://[::1]:8443/v1", false)).toBe(true);
    expect(endpointValid("https://speech.example.net:99999/v1", false)).toBe(false);
    expect(endpointValid("https://speech.example.net:+80/v1", false)).toBe(false);
    expect(endpointValid("https://speech.example.net/%zz", false)).toBe(false);
    expect(endpointValid("HTTPS://speech.example.net/v1", false)).toBe(true);
  });
});
