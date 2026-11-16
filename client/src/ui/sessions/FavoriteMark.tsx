import { Show } from "solid-js";
import { favorite } from "../../store/favorites";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import type { Session } from "./types";

export function FavoriteMark(props: { session: Session }) {
  return <Show when={favorite(props.session)}><span class="sc-favorite" role="img" aria-label={tr("Favorite")} title={tr("Favorite")}><Icon name="star" size={12} /></span></Show>;
}
