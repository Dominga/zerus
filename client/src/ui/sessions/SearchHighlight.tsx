import { For, createContext } from "solid-js";

/**
 * The query a shown search hit highlights in its message and tool output
 * (zerus-5wmh.13.17.5, zerus-2r73y); empty elsewhere. An accessor: a
 * provider's value is read once, and the hit shown can change its query
 * without remounting (zerus-2r73y.1).
 */
export const HighlightQuery = createContext<() => string>(() => "");

export function SearchHighlight(props: { text: string; query: string }) {
  const parts = () => props.query ? props.text.split(new RegExp(`(${props.query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "giu")) : [props.text];
  return <For each={parts()}>{(part, index) => index() % 2 ? <mark>{part}</mark> : part}</For>;
}
