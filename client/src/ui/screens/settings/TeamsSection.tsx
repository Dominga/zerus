import type { JSX } from "solid-js";
import { tr } from "../../../i18n";
import { setPage } from "../../../store/ui";
import { setSetting, settings } from "../../../store/settings";
import { noticeMode, DEFAULT_NOTICE_MODE, type NoticeMode } from "../../sessions/leadNotices";
import { Row, Select } from "./ui";

/** Settings > Teams: the ways Qt reaches the Teams screens. */
export function TeamsSection(): JSX.Element {
  return (
    <div class="set-section">
      <h2>{tr("Teams")}</h2>
      <p class="set-note">{tr("New team, the plan's settings and the team catalog are on the team's own bar; the catalog is also here.")}</p>
      <div class="set-actions">
        <button type="button" class="btn small" onClick={() => setPage("newteam")}>{tr("New team")}</button>
        <button type="button" class="btn ghost small" onClick={() => setPage("catalog")}>{tr("Team catalog")}</button>
        <button type="button" class="btn ghost small" onClick={() => setPage("designer")}>{tr("Team designer")}</button>
        <button type="button" class="btn ghost small" onClick={() => setPage("sessions")}>{tr("Open the Team tab")}</button>
      </div>
      {/* Task 244: how a lead session shows the team's notices, unless the session chose its own mode. */}
      <Row label={tr("Team notices in a lead's session")} hint={tr("Collapsed shows each notice and the lead's reply to it as one folded line and never notifies you; Hidden shows neither. The lead still receives everything, and what waits for you is never hidden.")}>
        <Select id="lead-notices" value={noticeMode(settings["teams/leadNotices"]) ?? DEFAULT_NOTICE_MODE}
          options={[{ value: "full", label: tr("Full") }, { value: "collapsed", label: tr("Collapsed") }, { value: "silent", label: tr("Hidden") }]}
          onChange={(value) => { void setSetting("teams/leadNotices", value as NoticeMode); }} />
      </Row>
    </div>
  );
}
