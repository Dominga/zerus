import { describe, expect, it } from "vitest";
import { MESSAGE_BYTES, PATH_BYTES, insertReference, pathProblem, referenceText, remotePath } from "./file-reference";

// Qt parity (I5a): MessageComposer::referenceFile's checks and insertion.

const CONTROL = "Enter a file path without line breaks or control characters.";

describe("Reference file", () => {
  it("refuses an empty or oversized path and control characters", () => {
    expect(pathProblem("/home/user/notes.md")).toBe("");
    expect(pathProblem("C:\\Users\\user\\My notes.md")).toBe("");
    expect(pathProblem("")).toBe(CONTROL);
    expect(pathProblem("/tmp/a\nb")).toBe(CONTROL);
    expect(pathProblem("/tmp/a\tb")).toBe(CONTROL);
    expect(pathProblem("/tmp/a\u0085b")).toBe(CONTROL);
    expect(pathProblem("/tmp/a\u007fb")).toBe(CONTROL);
    expect(pathProblem(`/${"a".repeat(PATH_BYTES - 1)}`)).toBe("");
    expect(pathProblem(`/${"a".repeat(PATH_BYTES)}`)).toBe(CONTROL);
    // Bytes, not characters: 11 000 three-byte characters exceed 32 KiB.
    expect(pathProblem(`/${"\u4e00".repeat(11_000)}`)).toBe(CONTROL);
  });

  it("takes a remote path that is absolute on its computer or under its home", () => {
    for (const path of ["/srv/app/main.rs", "~/notes.md", "C:\\work\\a.txt", "c:/work/a.txt", "\\\\server\\share\\a.txt", "//server/share/a.txt"]) expect(remotePath(path), path).toBe(true);
    for (const path of ["notes.md", "./notes.md", "~notes", "~", "C:work", "\\work\\a.txt", "//", "\\\\?\\C:\\a.txt"]) expect(remotePath(path), path).toBe(false);
  });

  it("quotes the path as JSON", () => {
    expect(referenceText("/home/user/My \"draft\".md")).toBe("Use the existing file at \"/home/user/My \\\"draft\\\".md\".");
    expect(referenceText("C:\\work\\a.txt")).toBe("Use the existing file at \"C:\\\\work\\\\a.txt\".");
  });

  it("replaces the selection, spaced from the text beside it, with the caret after it", () => {
    const reference = referenceText("/a.txt");
    expect(insertReference("", 0, 0, "/a.txt")).toEqual({ text: reference, caret: reference.length });
    expect(insertReference("Read", 4, 4, "/a.txt")).toEqual({ text: `Read ${reference}`, caret: 5 + reference.length });
    expect(insertReference("Read then", 4, 4, "/a.txt")).toEqual({ text: `Read ${reference} then`, caret: 5 + reference.length });
    expect(insertReference("Read  then", 5, 5, "/a.txt")).toEqual({ text: `Read ${reference} then`, caret: 5 + reference.length });
    // The selection goes, either way it was made.
    expect(insertReference("Read THIS now", 9, 5, "/a.txt")).toEqual({ text: `Read ${reference} now`, caret: 5 + reference.length });
    expect(insertReference("x", 0, 0, "/a.txt")).toEqual({ text: `${reference} x`, caret: reference.length + 1 });
  });

  it("keeps the message within 64 KiB", () => {
    const reference = referenceText("/a.txt");
    const room = "a".repeat(MESSAGE_BYTES - reference.length - 1);
    expect(insertReference(`${room} `, room.length + 1, room.length + 1, "/a.txt")).toMatchObject({ caret: MESSAGE_BYTES });
    expect(insertReference(`${room}  `, room.length + 2, room.length + 2, "/a.txt")).toEqual({ error: "Message exceeds 64 KiB." });
  });
});
