// @vitest-environment happy-dom
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { OutgoingMessage } from "./Activity";
import { loadMarkdown } from "./markdown";
import type { ActivityItem } from "./types";
import { DIFF, MESSAGE } from "./unfenced-diff.fixture";

// zerus-jj9jp: an Activity row (the shared message body of user, agent and
// assistant rows) shows an unfenced unified diff as one code block.
let dispose: (() => void) | undefined;
beforeAll(async () => { await loadMarkdown(); });
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

describe("an Activity row with an unfenced diff", () => {
  for (const kind of ["user", "agent"] as const) {
    it(`renders it in a ${kind} row as one code block`, () => {
      const item: ActivityItem = { id: `m-${kind}`, kind, at: "10:00", body: MESSAGE };
      const host = document.createElement("div");
      document.body.append(host);
      dispose = render(() => createComponent(OutgoingMessage, { item }), host);
      const body = host.querySelector(".act-body")!;
      expect([...body.querySelectorAll("pre.act-code")].map((pre) => pre.textContent)).toEqual([DIFF]);
      expect(body.querySelectorAll("li")).toHaveLength(2);
    });
  }
});
