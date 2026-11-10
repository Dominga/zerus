import { For, Show, createEffect, createMemo, createResource, createSignal, createUniqueId, on } from "solid-js";
import { teamHelpLanguage } from "../../store/teamHelp";
import { Icon } from "../lib/icons";
import { TextInput } from "../lib/TextField";
import { Body } from "../sessions/MarkdownBody";
import { loadMarkdown } from "../sessions/markdown";
import { CHAPTERS, chapterLabel, findChapters, loadGuide, type ChapterId } from "./help/guide";
import "../styles/team-guide.css";

export function TeamGuideReader(props: { inline?: boolean }) {
  const language = teamHelpLanguage;
  const ru = () => language() === "ru";
  const [guide, { refetch }] = createResource(language, async (lang) => { await loadMarkdown(); return loadGuide(lang); });
  const document = () => guide.error ? undefined : guide();
  const [query, setQuery] = createSignal("");
  const [chapter, setChapter] = createSignal<ChapterId>("prepare");
  const [cookie, setCookie] = createSignal(0);
  const id = createUniqueId();
  let article: HTMLElement | undefined;
  let navigation: HTMLElement | undefined;
  const results = createMemo(() => findChapters(document()?.chapters ?? [], query()));
  const current = createMemo(() => results().find((item) => item.id === chapter()) ?? results()[0]);
  const position = () => results().findIndex((item) => item.id === current()?.id);
  const choose = (next: ChapterId) => {
    setChapter(next);
    queueMicrotask(() => {
      navigation?.querySelector<HTMLElement>('[aria-current="page"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
      article?.scrollTo({ top: 0 });
      article?.focus({ preventScroll: true });
    });
  };
  createEffect(on(() => current()?.id, () => article?.scrollTo({ top: 0 })));
  const cookieMessage = () => {
    const lines = ru()
      ? ["Лид получил печеньку. Новых фаз не добавилось.", "Крошки убраны в отдельный worktree.", "QA проверил печеньку. Хруст подтверждён."]
      : ["The lead got a cookie. No extra phases were created.", "The crumbs have their own worktree.", "QA tested the cookie. Crunch confirmed."];
    return cookie() ? lines[(cookie() - 1) % lines.length] : "";
  };
  return (
    <section class="team-guide" classList={{ "tg-inline": props.inline }} lang={language()} aria-label={ru() ? "Как работать с командой" : "Working with a team"}>
      <header class="tg-hero">
        <h2 class="tg-title"><Icon name="file-text" size={16} />{ru() ? "Путеводитель по командам" : "Team field guide"}</h2>
        <div class="tg-hero-foot"><span>{ru() ? "11 разделов" : "11 chapters"}</span><span>{ru() ? "3 варианта команды" : "3 team lineups"}</span></div>
      </header>
      <div class="tg-search">
        <Icon name="search" size={16} />
        <TextInput type="search" value={query()} onInput={(event) => setQuery(event.currentTarget.value)}
          placeholder={ru() ? "Найти тему, например: Free" : "Find a topic, e.g. Free"}
          aria-label={ru() ? "Поиск по справке" : "Search the guide"} />
      </div>
      <Show when={guide.error}>
        <div class="tg-empty" role="alert"><p>{ru() ? "Не удалось открыть справку." : "The guide could not be loaded."}</p>
          <button type="button" class="btn" onClick={() => void refetch()}>{ru() ? "Повторить" : "Try again"}</button></div>
      </Show>
      <Show when={document()} fallback={<Show when={!guide.error}><p class="tg-empty" role="status">{ru() ? "Открываем справку…" : "Opening the guide…"}</p></Show>}>
        <div class="tg-layout">
          <nav class="tg-nav" ref={navigation} aria-label={ru() ? "Разделы справки" : "Guide chapters"}>
            <For each={results()}>{(item) => (
              <button type="button" classList={{ on: current()?.id === item.id }} aria-current={current()?.id === item.id ? "page" : undefined}
                aria-controls={`${id}-article`} data-guide-chapter={item.id} title={item.summary} onClick={() => choose(item.id)}>
                <Icon name={item.icon} size={16} /><span>{chapterLabel(item.id, language())}</span>
                <span class="tg-number">{String(CHAPTERS.findIndex((row) => row[0] === item.id) + 1).padStart(2, "0")}</span>
              </button>
            )}</For>
          </nav>
          <article class="tg-article" ref={article} id={`${id}-article`} tabindex="-1" aria-labelledby={current() ? `${id}-chapter` : undefined}>
            <Show when={current()} fallback={<p class="tg-empty" role="status">{ru() ? "Такой темы нет. Попробуй другое слово." : "No matching chapter. Try another word."}</p>}>
              {(item) => <>
                <div class="tg-chapter-kicker">{item().summary}</div>
                <h2 id={`${id}-chapter`}>{item().title}</h2>
                <div class="tg-prose"><Body text={item().id === "prepare" ? `${document()!.intro}\n\n${item().body}` : item().body} /></div>
                <footer class="tg-pager">
                  <button type="button" class="btn ghost small" disabled={position() <= 0} onClick={() => choose(results()[position() - 1].id)}><Icon name="chevron-left" size={14} />{ru() ? "Назад" : "Previous"}</button>
                  <span>{position() + 1} / {results().length}</span>
                  <button type="button" class="btn small" disabled={position() >= results().length - 1} onClick={() => choose(results()[position() + 1].id)}>{ru() ? "Дальше" : "Next"}<Icon name="chevron-right" size={14} /></button>
                </footer>
              </>}
            </Show>
          </article>
        </div>
      </Show>
      <footer class="tg-cookie"><button type="button" onClick={() => setCookie((n) => n % 3 + 1)} aria-label={ru() ? "Дать лиду виртуальную печеньку" : "Give the lead a virtual cookie"}>
        <span aria-hidden="true">🍪</span>{ru() ? "Печенька лиду" : "Cookie for the lead"}</button><span role="status" aria-live="polite">{cookieMessage()}</span></footer>
    </section>
  );
}
