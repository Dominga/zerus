import type { HelpLanguage } from "../../../store/teamHelp";

export const CHAPTERS = [
  ["prepare", "sparkles", "Prepare the plan", "Подготовь план", "A clear goal and independent review", "Цель и независимое кросс-ревью"],
  ["create", "users", "Create the team", "Создай команду", "From the lead to the first task", "От лида до первой задачи"],
  ["lineup", "users", "Choose a lineup", "Подбери состав", "Three practical starting points", "Три варианта для старта"],
  ["folders", "folder", "Working folders", "Рабочие папки", "Worktrees or one shared folder", "Worktree или одна папка"],
  ["broker", "cpu", "Build broker", "Билд-брокер", "Build together without a traffic jam", "Сборки без толкучки"],
  ["modes", "git-branch", "Standard and Free", "Standard и Free", "Phases, approvals and a steady stream", "Фазы, приёмка и поток задач"],
  ["tests", "list-todo", "Tests and queues", "Тесты и очереди", "Run the checks that matter", "Запускай нужные проверки"],
  ["memory", "file-text", "Team memory", "Память команды", "Keep decisions beyond one conversation", "Решения переживут контекст"],
  ["workspace", "layout-dashboard", "Follow the work", "Следи за работой", "Views, task cards and your decisions", "Виды плана, задачи и твои решения"],
  ["profile", "activity", "Read the profiler", "Читай профайлер", "Time, tokens and bottlenecks", "Время, токены и задержки"],
  ["settings", "settings", "Team settings", "Настройки команды", "Roles, models and planning rules", "Роли, модели и правила"],
] as const;
export type ChapterId = typeof CHAPTERS[number][0];
export interface GuideChapter { id: ChapterId; title: string; body: string; icon: string; summary: string }
export interface GuideDocument { title: string; intro: string; chapters: GuideChapter[] }

/** Fenced command comments must never become chapters. IDs survive translation. */
export function splitGuide(source: string, language: HelpLanguage): GuideDocument {
  let fence = false;
  let title = "";
  const intro: string[] = [];
  const sections: { title: string; lines: string[] }[] = [];
  for (const line of source.replace(/\r\n/g, "\n").split("\n")) {
    if (/^```/.test(line)) fence = !fence;
    if (!fence && line.startsWith("# ")) title = line.slice(2);
    else if (!fence && line.startsWith("## ")) sections.push({ title: line.slice(3), lines: [] });
    else (sections.at(-1)?.lines ?? intro).push(line);
  }
  if (sections.length !== CHAPTERS.length) throw new Error("The team guide has an incomplete chapter list.");
  return { title, intro: intro.join("\n").trim(), chapters: sections.map((section, i) => ({
    id: CHAPTERS[i][0], icon: CHAPTERS[i][1], title: section.title,
    summary: CHAPTERS[i][language === "ru" ? 5 : 4], body: section.lines.join("\n").trim(),
  })) };
}

export function findChapters(chapters: GuideChapter[], query: string): GuideChapter[] {
  const words = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return chapters.filter((chapter) => words.every((word) => `${chapter.title} ${chapter.body}`.toLocaleLowerCase().includes(word)));
}

export const chapterLabel = (id: ChapterId, language: HelpLanguage) => CHAPTERS.find((row) => row[0] === id)![language === "ru" ? 3 : 2];
export async function loadGuide(language: HelpLanguage): Promise<GuideDocument> {
  const source = language === "ru" ? await import("./team-guide.ru.md?raw") : await import("./team-guide.en.md?raw");
  return splitGuide(source.default, language);
}
