import { createSignal } from "solid-js";
import {
  onSessionFileChanged,
  sessionFileAct,
  sessionFileClose,
  sessionFileDialog,
  sessionFileDownload,
  sessionFileExternal,
  sessionFileSave,
  type SessionFileView,
} from "../../bridge";
import { toast } from "../../store/ui";
import type { Session } from "./types";

const [sessionFile, setSessionFile] = createSignal<SessionFileView>();
export { sessionFile };

onSessionFileChanged((view) => {
  if (sessionFile()?.id === view.id) setSessionFile(view.closed ? undefined : view);
});

function report(error: unknown) {
  toast("File reference", String(error));
}

export function openExternal(url: string) {
  void sessionFileExternal(url).catch(report);
}

export function openSessionFile(session: Session, href: string) {
  void sessionFileDialog(session.computer, session.catalogKey, href, session.cwd).then(setSessionFile).catch(report);
}

export function closeSessionFile() {
  const current = sessionFile();
  setSessionFile(undefined);
  if (current) void sessionFileClose(current.id).catch(report);
}

export function actOnSessionFile(action: "open_file" | "open_folder" | "open_copy" | "show_file") {
  const current = sessionFile();
  if (current) void sessionFileAct(current.id, action).then(setSessionFile).catch(report);
}

export function downloadSessionFile(open: boolean) {
  const current = sessionFile();
  if (current) void sessionFileDownload(current.id, open).then(setSessionFile).catch(report);
}

export function saveSessionFile() {
  const current = sessionFile();
  if (current) void sessionFileSave(current.id).then(setSessionFile).catch(report);
}
