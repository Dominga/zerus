import { describe, expect, it } from "vitest";
import type { AttachmentScope } from "../../store/activitySource";
import type { ActivityEvent, AttachmentRef, Fetched } from "../../types";
import { readCards } from "./attachment-card";
import { attachmentEchoes, buildTranscript } from "./transcript";
import type { ActivityItem } from "./types";

const imagePath = "/home/example/.local/state/hgs/sessions/attachments/message-ab12cd/01-pasted-image.png";
const imageEcho = `[Image #1] (Inspect the attached image at ${JSON.stringify(imagePath)})`;
const image = { name: "pasted-image.png", mime: "image/png", bytes: 68, reference: "[Image #1]", request_id: "req-image", index: 0 };
const scope: AttachmentScope = { computer: "studio", name: "claude/sample-project/review", conversation_id: "conv-1", archive_id: null };

function receipt(raw: string, detail: string, files: ActivityEvent["attachments"]): ActivityEvent {
  return { source: "hgs_delivery", type: "UserPromptSubmit", message_id: "req-image", detail, submitted_text: raw, attachments: files };
}

function shown(raw: string, receipts: ActivityEvent[] = [receipt(raw, "Check the wording", [image])]): ActivityItem[] {
  return buildTranscript([{ seq: 1, type: "UserPromptSubmit", detail: raw, at: 100 }], { attachment_messages: receipts }) as ActivityItem[];
}

describe("native attachment prompt echoes", () => {
  it("recognises numbered image and file locations, including JSON-escaped Windows paths", () => {
    const windows = "C:\\Users\\example\\.local\\state\\hgs\\sessions\\attachments\\message-cd34ef\\02-notes.txt";
    const raw = `${imageEcho} and [File #2] (Read the attached file at ${JSON.stringify(windows)})`;
    expect(attachmentEchoes(raw)).toEqual([
      { reference: "[Image #1]", kind: "Image", path: imagePath },
      { reference: "[File #2]", kind: "File", path: windows },
    ]);
    expect(attachmentEchoes("[Image #1] (Read the attached file at \"/tmp/image.png\")")).toEqual([]);
    expect(attachmentEchoes("[Image #1] (Inspect the attached image at \"broken\\\")")).toEqual([]);
  });

  it("replaces the owner's exact image echo with its card and keeps the message text", () => {
    const raw = `${imageEcho}\nCheck the wording`;
    const items = shown(raw, [receipt(raw, "[Image #1]\nCheck the wording", [image])]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      kind: "user", body: "[Image #1]\nCheck the wording", echoBody: raw,
      attachments: [{ name: "pasted-image.png", request_id: "req-image", index: 0, image: true }],
    });
  });

  it("reconciles numbered images and files in one message", () => {
    const filePath = "/home/example/.local/state/hgs/sessions/attachments/message-cd34ef/02-notes.txt";
    const file = { name: "notes.txt", mime: "text/plain", bytes: 12, reference: "[File #2]", request_id: "req-both", index: 1 };
    const both = [{ ...image, request_id: "req-both" }, file];
    const raw = `Compare ${imageEcho} with [File #2] (Read the attached file at ${JSON.stringify(filePath)})`;
    const items = shown(raw, [receipt(raw, "Compare [Image #1] with [File #2]", both)]);
    expect(items).toHaveLength(1);
    expect(items[0].body).toBe("Compare [Image #1] with [File #2]");
    expect(items[0].attachments?.map((file) => [file.request_id, file.index, file.image])).toEqual([["req-both", 0, true], ["req-both", 1, false]]);
  });

  it("keeps unknown, wrongly located and ambiguous echoes as raw text", () => {
    const raw = `${imageEcho}\nCheck the wording`;
    expect(shown(raw, [])[0]).toMatchObject({ body: raw, echoBody: undefined, attachments: undefined });
    expect(shown(raw, [receipt("a different submission", "Check the wording", [image])])[0].body).toBe(raw);
    const elsewhere = raw.replace("/sessions/attachments/", "/elsewhere/");
    expect(shown(elsewhere, [receipt(elsewhere, "Check the wording", [image])])[0].body).toBe(elsewhere);
    const two = buildTranscript([
      { seq: 1, type: "UserPromptSubmit", detail: raw },
      { seq: 2, type: "TurnStarted", detail: raw },
    ], { attachment_messages: [receipt(raw, "Check the wording", [image])] }) as ActivityItem[];
    expect(two.map((item) => item.body)).toEqual([raw, raw]);
    expect(two.every((item) => !item.attachments)).toBe(true);
  });
});

describe("reconciled attachment reads", () => {
  it("uses the shown conversation's computer and guarded request identities", async () => {
    const raw = `${imageEcho}\nCheck the wording`;
    const files = shown(raw)[0].attachments!;
    const calls: [string, AttachmentRef][] = [];
    const fetched: Fetched = { path: "cache/digest/pasted-image.png", name: "pasted-image.png", mime: "image/png", bytes: 68, thumbnail: true };
    const answer = await readCards(files, scope, async (computer, ref) => {
      calls.push([computer, ref]);
      return fetched;
    });
    expect(answer).toEqual([fetched]);
    expect(calls).toEqual([["studio", {
      name: scope.name, request_id: "req-image", index: 0, conversation_id: "conv-1", archive_id: null, agent_id: null,
    }]]);
    expect(calls[0][1]).not.toHaveProperty("path");
  });

  it("reports an unreadable or unscoped file so the view can restore the raw echo", async () => {
    const raw = `${imageEcho}\nCheck the wording`;
    const files = shown(raw)[0].attachments!;
    expect(await readCards(files, scope, async () => { throw "Attachment receipt is unavailable"; })).toBeUndefined();
    expect(await readCards(files, undefined, async () => { throw "must not read"; })).toBeUndefined();
    expect(await readCards([{ ...files[0], request_id: "" }], scope, async () => { throw "must not read"; })).toBeUndefined();
  });
});
