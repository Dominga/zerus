import { describe, expect, it } from "vitest";
import { MESSAGE_BYTES, referenceSentence, remoteForm, sameDraft, styleOfPlatform, wellFormed, withReference } from "./file-reference";

describe("Reference an existing file: path rules (Qt MessageComposer.cpp:919-931)", () => {
  it("refuses an empty path, line breaks, other controls and more than 32 KiB", () => {
    expect(wellFormed("/srv/report.pdf")).toBe(true);
    expect(wellFormed("C:\\Users\\example\\Отчёт.pdf")).toBe(true);
    for (const bad of ["", "/srv/a\nb", "/srv/a\rb", "/srv/a\tb", "/srv/\u0007", "/srv/\u007f", "/srv/\u0085"]) expect(wellFormed(bad), JSON.stringify(bad)).toBe(false);
    expect(wellFormed("/" + "x".repeat(32 << 10))).toBe(false);
    expect(wellFormed("/" + "x".repeat((32 << 10) - 1))).toBe(true);
    // 32 KiB of UTF-8, not of characters.
    expect(wellFormed("/" + "ж".repeat(16 << 10))).toBe(false);
  });

  it("checks a remote path's form for the computer's platform, and only its form", () => {
    expect(remoteForm("/srv/report.pdf", "posix")).toBe(true);
    expect(remoteForm("~/report.pdf", "posix")).toBe(true);
    expect(remoteForm("//server/share/x", "posix")).toBe(false);
    expect(remoteForm("C:\\work\\x.txt", "posix")).toBe(false);
    expect(remoteForm("relative/x.txt", "posix")).toBe(false);
    expect(remoteForm("C:\\work\\x.txt", "windows")).toBe(true);
    expect(remoteForm("\\\\server\\share\\x.txt", "windows")).toBe(true);
    expect(remoteForm("~/x.txt", "windows")).toBe(true);
    expect(remoteForm("/srv/x", "windows")).toBe(false);
    expect(remoteForm("C:work", "windows")).toBe(false);
    // Before the computer reports its platform, either form, as Qt.
    expect(remoteForm("/srv/x", undefined)).toBe(true);
    expect(remoteForm("C:\\work\\x.txt", undefined)).toBe(true);
    expect(remoteForm("//server/share/x", undefined)).toBe(true);
    expect(remoteForm("relative", undefined)).toBe(false);
  });

  it("takes the style from the reported platform", () => {
    expect(styleOfPlatform("windows")).toBe("windows");
    expect(styleOfPlatform("linux")).toBe("posix");
    expect(styleOfPlatform("macos")).toBe("posix");
    expect(styleOfPlatform(null)).toBeUndefined();
    expect(styleOfPlatform("")).toBeUndefined();
  });
});

describe("Reference an existing file: the draft", () => {
  it("writes Qt's sentence with the path JSON-quoted", () => {
    expect(referenceSentence("/srv/My \"quoted\" file.txt")).toBe("Use the existing file at \"/srv/My \\\"quoted\\\" file.txt\".");
    expect(referenceSentence("C:\\work\\a b.txt")).toBe("Use the existing file at \"C:\\\\work\\\\a b.txt\".");
  });

  it("replaces the selection and adds a space only where the sentence touches text", () => {
    const sentence = "Use the existing file at \"/x\".";
    expect(withReference("", 0, 0, sentence)).toEqual({ text: sentence, cursor: sentence.length });
    expect(withReference("See", 3, 3, sentence)).toEqual({ text: `See ${sentence}`, cursor: 4 + sentence.length });
    expect(withReference("See \nthen", 4, 4, sentence)).toEqual({ text: `See ${sentence}\nthen`, cursor: 4 + sentence.length });
    expect(withReference("aXXb", 1, 3, sentence)).toEqual({ text: `a ${sentence} b`, cursor: 2 + sentence.length + 1 });
    // A backwards selection is the same range.
    expect(withReference("aXXb", 3, 1, sentence)?.text).toBe(`a ${sentence} b`);
  });

  it("refuses a draft past the 64 KiB message limit", () => {
    const sentence = "Use the existing file at \"/x\".";
    // After a trailing space no spacing is added: exactly the limit fits.
    const fits = "x".repeat(MESSAGE_BYTES - sentence.length - 1) + " ";
    expect(withReference(fits, fits.length, fits.length, sentence)?.text.length).toBe(MESSAGE_BYTES);
    const over = "x" + fits;
    expect(withReference(over, over.length, over.length, sentence)).toBeUndefined();
  });

  it("goes only into the draft the dialog opened on, while it is free", () => {
    const bound = { handle: 3, key: "draft", epoch: 7 };
    const view = { handle: 3, key: "draft", text_epoch: 7 };
    expect(sameDraft(view, bound, false)).toBe(true);
    expect(sameDraft(view, bound, true)).toBe(false);
    expect(sameDraft(undefined, bound, false)).toBe(false);
    expect(sameDraft({ ...view, handle: 4 }, bound, false)).toBe(false);
    expect(sameDraft({ ...view, key: "other" }, bound, false)).toBe(false);
    expect(sameDraft({ ...view, text_epoch: 8 }, bound, false)).toBe(false);
  });
});
