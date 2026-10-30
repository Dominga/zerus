import { For } from "solid-js";

export function SearchHighlight(props: { text: string; query: string }) {
  const parts = () => props.query ? props.text.split(new RegExp(`(${props.query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "giu")) : [props.text];
  return <For each={parts()}>{(part, index) => index() % 2 ? <mark>{part}</mark> : part}</For>;
}
