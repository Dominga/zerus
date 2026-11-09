import { describe, expect, it } from "vitest";

// Task 217: the lead's open proposals are cards in the Activity column right
// above the composer, next to the question card (Qt SessionsWindow.cpp:1083-1084:
// the proposal cards and then the question card go into the activity's input
// column, under the transcript). The Activity is drawn in SessionDetail.tsx;
// the cards carry the column's own width class, like the question card.
describe("the lead's proposal cards' place in the Activity", () => {
  it("sits between the queue and the question card, above the composer", async () => {
    const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
    const read = (path: string) => fs.readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
    const detail = read("./SessionDetail.tsx");
    const at = (text: string) => detail.indexOf(text);
    const queue = at("<QueuePanel />"), proposals = at("<LeadProposals />"), question = at("<QuestionSlot />");
    const composer = at('<div class="cmp-col"><Composer /></div>');
    expect(queue).toBeGreaterThan(-1);
    expect(proposals).toBeGreaterThan(queue);
    expect(proposals).toBeLessThan(question);
    expect(question).toBeLessThan(composer);
    // The composer follows the question card inside the Activity, as in Qt.
    expect(detail.slice(question, composer)).toContain("</Show>");
    expect(detail.slice(proposals, composer)).not.toContain("QueuePanel");
  });

  it("draws the cards and the question card in one column width, as Qt's input column does", async () => {
    const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
    const cards = fs.readFileSync(new URL("../teams/ProposalCards.tsx", import.meta.url), "utf8").replace(/\r\n/g, "\n");
    // Task 317 (F7): lead-proposals keeps them out of the column's shrink.
    expect(cards).toContain('<div class="cmp-col lead-proposals"><TeamProposals computer={row()!.computer} team={view()} /></div>');
  });
});
