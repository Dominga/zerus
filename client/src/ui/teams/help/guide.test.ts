import { describe, expect, it } from "vitest";
import { guideLanguage, setTeamHelpSystemLanguage, teamHelpLanguage } from "../../../store/teamHelp";
import { numberText, setSystemLocale } from "../../sessions/usage-number";
import { CHAPTERS, findChapters, splitGuide } from "./guide";
import en from "./team-guide.en.md?raw";
import ru from "./team-guide.ru.md?raw";
import { resolveIcon } from "../../lib/icons";

describe("the team's system-language guide", () => {
  it.each([
    ["ru-RU", "en-US", "ru"], ["RU_ru.UTF-8", "en", "ru"], ["ru", "en", "ru"],
    ["en-GB", "ru-RU", "en"], ["", "ru-KZ", "ru"], ["", "en-US", "en"],
    ["de-DE", "ru-RU", "en"], ["russian", "en", "en"],
  ])("uses display language %s before WebView %s", (os, browser, expected) => {
    expect(guideLanguage(os, browser)).toBe(expected);
  });

  it.each([
    ["en-US", "ru-RU", "en"],
    ["ru-RU", "en-US", "ru"],
  ])("keeps display language %s separate from regional format %s", (display, region, expected) => {
    try {
      setSystemLocale(region);
      setTeamHelpSystemLanguage(display);
      expect(teamHelpLanguage()).toBe(expected);
      expect(numberText(1234)).toBe(new Intl.NumberFormat(region).format(1234));
    } finally {
      setSystemLocale("");
      setTeamHelpSystemLanguage("");
    }
  });

  it("keeps the full approved document in matching translated chapters", () => {
    const english = splitGuide(en, "en"), russian = splitGuide(ru, "ru");
    expect(english.chapters.map((c) => c.id)).toEqual(CHAPTERS.map((c) => c[0]));
    expect(russian.chapters.map((c) => c.id)).toEqual(english.chapters.map((c) => c.id));
    for (const guide of [english, russian]) {
      expect(guide.chapters.every((chapter) => !!resolveIcon(chapter.icon).html)).toBe(true);
      expect(guide.intro.length).toBeGreaterThan(150);
      expect(guide.chapters.every((chapter) => chapter.body.length > 100)).toBe(true);
      expect(guide.chapters.find((c) => c.id === "tests")?.body).toContain('"action":"submit"');
      expect(guide.chapters.find((c) => c.id === "tests")?.body).toContain("# ");
      expect(guide.chapters.find((c) => c.id === "profile")?.body).toContain("Overrun");
      expect(guide.chapters.find((c) => c.id === "modes")?.body).toContain("Auto-accept drafts");
    }
  });

  it("searches the full text, including commands and nested headings", () => {
    const guide = splitGuide(en, "en");
    expect(findChapters(guide.chapters, "request_id").map((c) => c.id)).toEqual(["tests"]);
    expect(findChapters(guide.chapters, " worktree CRUMBS ")).toEqual([]);
    expect(findChapters(guide.chapters, " ")).toHaveLength(CHAPTERS.length);
    expect(findChapters(splitGuide(ru, "ru").chapters, "перерасход").some((c) => c.id === "profile")).toBe(true);
  });
});
