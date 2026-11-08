import { For, Show, createContext, createMemo, useContext, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";
import { tr } from "../../i18n";
import { markdownReady, markdownTree, type Inline, type MarkdownBlock } from "./markdown";
import { openExternal, openSessionFile } from "./sessionFiles";
import type { Session } from "./types";

// Inline Markdown stays text except its links: HTTP(S) uses the system
// opener, and local/file references open the session's File reference dialog.

export const ActivitySession = createContext<() => Session | undefined>();

function inlines(parts: Inline[], session?: () => Session | undefined): JSX.Element[] {
  return parts.map((part): JSX.Element => {
    switch (part.kind) {
      case "text": return part.text;
      case "code": return <code>{part.text}</code>;
      case "br": return <br />;
      case "bold": return <b>{inlines(part.children, session)}</b>;
      case "em": return <em>{inlines(part.children, session)}</em>;
      case "del": return <del>{inlines(part.children, session)}</del>;
      default: {
        const link = part;
        return <a href={link.href} title={link.href} draggable={false} onAuxClick={(event) => event.preventDefault()} onDragStart={(event) => event.preventDefault()} onClick={(event) => {
          event.preventDefault();
          if (link.kind === "external") openExternal(link.href);
          else {
            const current = session?.();
            if (current) openSessionFile(current, link.href);
          }
        }}>{inlines(link.children, session)}</a>;
      }
    }
  });
}

const HEADINGS = ["h1", "h2", "h3", "h4", "h5", "h6"] as const;

function blocks(list: MarkdownBlock[], session?: () => Session | undefined): JSX.Element[] {
  return list.map((b): JSX.Element => {
    switch (b.kind) {
      case "code": return <pre class="act-code mono">{b.text}</pre>;
      case "text": return <>{inlines(b.inline, session)}</>;
      case "heading": return <Dynamic component={HEADINGS[b.level - 1]} class="md-heading">{inlines(b.inline, session)}</Dynamic>;
      case "hr": return <hr class="md-rule" />;
      case "quote": return <blockquote class="md-quote">{blocks(b.blocks, session)}</blockquote>;
      case "list": {
        const items = b.items.map((item) => (
          <li classList={{ "md-task": item.task }}>
            <Show when={item.task}><input type="checkbox" class="md-check" checked={item.checked} disabled aria-label={item.checked ? tr("Done") : tr("Not done")} /></Show>
            {blocks(item.blocks, session)}
          </li>
        ));
        return b.ordered ? <ol class="md-list" start={b.start}>{items}</ol> : <ul class="md-list">{items}</ul>;
      }
      case "table": return (
        <div class="act-table-wrap" role="region" aria-label={tr("Message table")} tabindex="0">
          <table class="act-table" style={{ "--table-columns": b.header.length }}>
            <thead><tr>{b.header.map((cell, column) => <th scope="col" style={{ "text-align": b.align[column] }}>{inlines(cell, session)}</th>)}</tr></thead>
            <Show when={b.rows.length}><tbody>{b.rows.map((row) => <tr>{row.map((cell, column) => <td style={{ "text-align": b.align[column] }}>{inlines(cell, session)}</td>)}</tr>)}</tbody></Show>
          </table>
        </div>
      );
      default: return <p>{inlines(b.inline, session)}</p>;
    }
  });
}

export function Body(props: { text: string }) {
  const session = useContext(ActivitySession);
  // Rerenders once when the lazily loaded parser arrives; Activity places its
  // rows only after that, so placed rows keep their height.
  const tree = createMemo(() => { markdownReady(); return markdownTree(props.text); });
  return <>{blocks(tree(), session)}</>;
}
