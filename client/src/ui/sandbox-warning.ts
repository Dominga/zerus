import { tr } from "../i18n";

// Task #247: Qt's blocking sandbox warning (main.cpp:457): the reason this
// process runs inside another program's sandbox, what it breaks, and Quit
// Zerus / Continue anyway. A blocking box, not a toast.

export interface SandboxWarningCopy {
  title: string;
  text: string;
  quit: string;
  stay: string;
}

export function sandboxWarningCopy(reason: string): SandboxWarningCopy {
  return {
    title: tr("Zerus is in a sandbox"),
    text: `${reason}\n\n${tr('Sessions cannot start from here, and settings, folders and the log may be refused with "access denied". Quit, then start Zerus from the Start menu or Explorer.')}`,
    quit: tr("Quit Zerus"),
    stay: tr("Continue anyway"),
  };
}
