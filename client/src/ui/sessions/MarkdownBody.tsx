import { For, Show, createContext, createMemo, useContext, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";
import { tr } from "../../i18n";
import { markdownReady, markdownTree, type Inline, type MarkdownBlock } from "./markdown";
import { openExternal, openSessionFile } from "./sessionFiles";
import type { Session } from "./types";
import { HighlightQuery, SearchHighlight } from "./SearchHighlight";

// Inline Markdown stays text except its links: HTTP(S) uses the system
// opener, and local/file references open the session's File reference dialog.

export const ActivitySession = createContext<() => Session | undefined>();
export { HighlightQuery };
const marked = (text: string, query: string): JSX.Element => (query ? <SearchHighlight text={text} query={query} /> : text);

function inlines(parts: Inline[], session?: () => Session | undefined, query = ""): JSX.Element[] {
  return parts.map((part): JSX.Element => {
    switch (part.kind) {
      case "text": return marked(part.text, query);
      case "code": return <code>{marked(part.text, query)}</code>;
      case "br": return <br />;
      case "bold": return <b>{inlines(part.children, session, query)}</b>;
      case "em": return <em>{inlines(part.children, session, query)}</em>;
      case "del": return <del>{inlines(part.children, session, query)}</del>;
      default: {
        const link = part;
        return <a href={link.href} title={link.href} draggable={false} onAuxClick={(event) => event.preventDefault()} onDragStart={(event) => event.preventDefault()} onClick={(event) => {
          event.preventDefault();
          if (link.kind === "external") openExternal(link.href);
          else {
            const current = session?.();
            if (current) openSessionFile(current, link.href);
          }
        }}>{inlines(link.children, session, query)}</a>;
      }
    }
  });
}

const HEADINGS = ["h1", "h2", "h3", "h4", "h5", "h6"] as const;

function blocks(list: MarkdownBlock[], session?: () => Session | undefined, query = ""): JSX.Element[] {
  return list.map((b): JSX.Element => {
    switch (b.kind) {
      case "code": return <pre class="act-code mono">{marked(b.text, query)}</pre>;
      case "text": return <>{inlines(b.inline, session, query)}</>;
      case "heading": return <Dynamic component={HEADINGS[b.level - 1]} class="md-heading">{inlines(b.inline, session, query)}</Dynamic>;
      case "hr": return <hr class="md-rule" />;
      case "quote": return <blockquote class="md-quote">{blocks(b.blocks, session, query)}</blockquote>;
      case "list": {
        const items = b.items.map((item) => (
          <li classList={{ "md-task": item.task }}>
            <Show when={item.task}><input type="checkbox" class="md-check" checked={item.checked} disabled aria-label={item.checked ? tr("Done") : tr("Not done")} /></Show>
            {blocks(item.blocks, session, query)}
          </li>
        ));
        return b.ordered ? <ol class="md-list" start={b.start}>{items}</ol> : <ul class="md-list">{items}</ul>;
      }
      case "table": return (
        <div class="act-table-wrap" role="region" aria-label={tr("Message table")} tabindex="0">
          <table class="act-table" style={{ "--table-columns": b.header.length }}>
            <thead><tr>{b.header.map((cell, column) => <th scope="col" style={{ "text-align": b.align[column] }}>{inlines(cell, session, query)}</th>)}</tr></thead>
            <Show when={b.rows.length}><tbody>{b.rows.map((row) => <tr>{row.map((cell, column) => <td style={{ "text-align": b.align[column] }}>{inlines(cell, session, query)}</td>)}</tr>)}</tbody></Show>
          </table>
        </div>
      );
      default: return <p>{inlines(b.inline, session, query)}</p>;
    }
  });
}

export function Body(props: { text: string }) {
  const session = useContext(ActivitySession);
  const query = useContext(HighlightQuery);
  // Rerenders once when the lazily loaded parser arrives; Activity places its
  // rows only after that, so placed rows keep their height.
  const tree = createMemo(() => { markdownReady(); return markdownTree(props.text); });
  return <>{blocks(tree(), session, query())}</>;
}
