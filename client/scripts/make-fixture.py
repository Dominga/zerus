#!/usr/bin/env python3
"""Writes the synthetic replay fixtures of the client (stdlib only, deterministic).

fixtures/replay.ndjson       watch-v1 frames of this computer, for ZERUS_CLIENT_REPLAY
fixtures/replay-mac.ndjson   the same for a peer named "mac"
fixtures/replay-batches.json the same fleet as ready batches, for `npm run dev` in a browser

Every name, path and message is synthetic: reserved example domains and
paths under /home/example/ only. The frames follow crates/watch-protocol: a hello, a
catalog snapshot in pages, then deltas separated by heartbeats (the replay
pauses one interval per heartbeat and loops the deltas). Activity frames are
held by the replay and served on follow, as the stream does after a subscribe.
"""
import json
import pathlib
import random

ROOT = pathlib.Path(__file__).resolve().parent.parent / "fixtures"
INTERVAL_MS = 900
BASE_AT = 1_793_000_000.0  # late October 2026, UTC

rng = random.Random(20261028)
seq_counter = [0]


def next_seq():
    seq_counter[0] += 1
    return seq_counter[0]


PROJECTS = {
    "sample-project": ["review", "api-refactor", "pagination", "docs-pass", "flaky-test", "release-notes", "i18n", "perf-budget"],
    "aurora": ["checkout-form", "orders-v3", "image-runner", "migrations", "search-index", "a11y-audit", "theme-tokens"],
    "orders-api": ["rate-limits", "webhooks", "billing-export", "retry-queue", "openapi"],
    "docs-site": ["landing", "changelog", "api-reference", "examples"],
    "infra": ["ci-cache", "runner-images", "backup-check", "dns-move", "metrics"],
}
MAC_PROJECTS = {"ios-app": ["widgets", "push-setup", "testflight"], "aurora": ["safari-fixes", "keyboard-nav"]}
AGENTS = ["claude", "claude", "codex", "claude", "kimi", "codex", "opencode", "deepseek"]
MODELS = {"claude": ("opus", "high"), "codex": ("gpt-5.5-codex", "medium"), "kimi": ("kimi-k3", ""), "opencode": ("qwen3-coder", ""), "deepseek": ("deepseek-v4", "")}
TOOLS = [("Read", "src/routes/orders.ts"), ("Grep", "\"TODO\" src"), ("Bash", "cargo test --workspace"), ("Edit", "src/pagination/cursor.rs"),
         ("Bash", "npm run check"), ("Write", "docs/pagination.md"), ("Bash", "git status --short"), ("Read", "README.md")]
PROMPTS = [
    "Keep the change small and tell me before anything destructive.",
    "Please look at the failing pagination test and fix the cursor round trip.",
    "Review the diff against main and list anything risky.",
    "Add a short section to the docs that explains the new limits.",
    "Run the checks again and summarise what still fails.",
]
REPLIES = [
    "I fixed the cursor round trip in `src/pagination/cursor.rs` and added a test for an empty page.\n\n- `cargo test` passes\n- no API change",
    "The diff touches three files. The only risky part is the retry loop, which now caps at five attempts.",
    "Done. The docs explain the limits with one example request against https://api.example.com/v1/orders.",
    "Two checks still fail on main as well, so they are not caused by this change:\n\n- `lint: unused import`\n- `docs: broken anchor`",
    "I kept the change scoped to the branch and left a note in the task.",
]
THOUGHTS = ["The test builds the cursor from the last row, so an empty page needs its own branch.", "Checking whether the retry cap is configurable before changing it."]


def session_rows(projects, host_seed):
    rows = []
    i = 0
    for project, tags in projects.items():
        for tag in tags:
            agent = AGENTS[(i + host_seed) % len(AGENTS)]
            model, effort = MODELS[agent]
            run = f"{host_seed:02d}{i:04d}-run"
            state = rng.choices(["idle", "working", "tool", "approval", "input", "error", "paused"], [10, 3, 3, 1, 1, 1, 2])[0]
            unread = state == "idle" and rng.random() < 0.3
            tool, target = TOOLS[i % len(TOOLS)]
            reply = f"reply-{run}"
            row = {
                "name": f"{agent}/{project}/{tag}",
                "agent": agent,
                "cmd": "dsh" if agent == "deepseek" else agent,
                "project": project,
                "tag": tag,
                "run_id": run,
                "conversation_id": f"conv-{run}",
                "cwd": f"/home/example/src/{project}",
                "git_branch": tag if i % 3 else "main",
                "model": model,
                "effort": effort,
                "phase": {"idle": "idle", "working": "working", "tool": "tool", "approval": "approval", "input": "input", "error": "error", "paused": "idle"}[state],
                "activity": "busy" if state in ("working", "tool") else "idle",
                "runtime_state": "paused" if state == "paused" else "live",
                "process_state": "stopped" if state == "paused" else "running",
                "conversation_state": "active",
                "activity_summary": f"{tool}: {target}" if state in ("working", "tool") else ("Waiting for approval: Bash" if state == "approval" else
                                    "Which layout should I keep?" if state == "input" else "Rate limit reached" if state == "error" else ""),
                "current_tool": tool if state == "tool" else "",
                "reply_id": reply,
                "read": {"reply_id": "" if unread else reply, "at": BASE_AT, "conversation_id": f"conv-{run}", "attention_signature": None},
                "last_event_at": BASE_AT + i * 37,
                "turn_started": BASE_AT + i * 37 - 120 if state in ("working", "tool") else 0,
            }
            rows.append(row)
            i += 1
    return rows


def key_of(row):
    return f"run:{row['run_id']}" if row.get("run_id") else f"name:{row['name']}"


def events_for(row, count):
    """A plausible conversation: prompts, tool runs, thinking and replies, oldest first."""
    events = []
    at = row["last_event_at"] - count * 20
    turn = 0
    while len(events) < count:
        at += 30
        prompt = PROMPTS[turn % len(PROMPTS)]
        events.append({"seq": next_seq(), "type": "UserPromptSubmit", "source": "claude_hooks", "detail": prompt, "at": at})
        if turn % 4 == 1:
            events.append({"seq": next_seq(), "type": "AgentThinking", "source": "claude_transcript", "detail": THOUGHTS[turn % len(THOUGHTS)], "at": at + 2})
        for t in range(1 + turn % 5):
            tool, target = TOOLS[(turn + t) % len(TOOLS)]
            events.append({"seq": next_seq(), "type": "PreToolUse", "source": "claude_hooks", "tool": tool, "detail": target, "at": at + 3 + t})
            failed = tool == "Bash" and turn % 7 == 3
            events.append({"seq": next_seq(), "type": "PostToolUseFailure" if failed else "PostToolUse", "source": "claude_hooks", "tool": tool,
                           "detail": "exit status 1" if failed else f"{target}: ok", "at": at + 4 + t})
            # Other sessions write to the same journal, so one session's seqs have gaps.
            seq_counter[0] += rng.randint(0, 3)
        if turn % 9 == 5:
            events.append({"seq": next_seq(), "type": "UserPromptSubmit", "source": "claude_hooks", "origin": "subagent_report", "sender": "explorer",
                           "detail": "Subagent report: the cursor module has two callers.", "report": "Both callers pass the last row; neither handles an empty page.", "at": at + 9})
        if turn % 11 == 7:
            events.append({"seq": next_seq(), "type": "UserPromptSubmit", "source": "claude_hooks", "origin": "peer_message", "sender": "codex/sample-project/review",
                           "detail": "Please rebase on main before you push; the CI cache moved.", "at": at + 10})
        events.append({"seq": next_seq(), "type": "Stop", "source": "claude_transcript", "detail": REPLIES[turn % len(REPLIES)], "at": at + 12})
        turn += 1
    return events[-count:] if len(events) > count else events


def state_for(row):
    return {"name": row["name"], "phase": row["phase"], "activity": row["activity"], "current_tool": row["current_tool"],
            "activity_summary": row["activity_summary"], "turn_started": row["turn_started"], "pending_questions": [], "history_truncated": False}


CAPTIONS = {"attention": None, "error": "Rate limit reached", "paused": "Paused", "ready": "Ready"}


def view_row(row, machine):
    """The view row the browser fixture carries, in the shape of client/core view.rs (a simplified projection)."""
    phase = row["phase"]
    unread = bool(row["reply_id"]) and row["read"]["reply_id"] != row["reply_id"]
    status = ("attention" if phase in ("approval", "input") else "error" if phase == "error" else "paused" if row["runtime_state"] == "paused"
              else "working" if row["activity"] == "busy" or phase in ("working", "tool", "compacting") else "ready")
    caption = ("Needs approval" if phase == "approval" else "Needs input" if phase == "input" else "Using a tool" if phase == "tool"
               else "Working" if status == "working" else "New reply" if status == "ready" and unread else CAPTIONS[status])
    return {"name": row["name"], "title": row["tag"], "agent": row["agent"], "status": status, "caption": caption, "phase": phase,
            "attention": "approval" if phase == "approval" else "question" if phase == "input" else None, "unread": unread, "muted": False,
            "machine": machine, "project": row["project"], "folder": row["cwd"].rstrip("/").rsplit("/", 1)[-1], "branch": row["git_branch"] or None,
            "model": row["model"] or None, "effort": row["effort"] or None, "summary": row["activity_summary"], "updated_at": row["last_event_at"],
            "turn_started": row["turn_started"] or None, "run_id": row["run_id"], "conversation_id": row["conversation_id"]}


def deltas_for(rows):
    """A loop of changes that ends where it starts: tool progress, a finished turn, a replaced run."""
    working = [r for r in rows if r["activity"] == "busy"][:3]
    idle = [r for r in rows if r["phase"] == "idle" and r["runtime_state"] == "live"][:2]
    steps = []
    for n in range(12):
        upserts = []
        for w in working:
            tool, target = TOOLS[(n + len(w["tag"])) % len(TOOLS)]
            upserts.append(dict(w, phase="tool", current_tool=tool, activity_summary=f"{tool}: {target}", last_event_at=w["last_event_at"] + n))
        if idle:
            flip = idle[0]
            busy = n % 6 < 3
            upserts.append(dict(flip, phase="working" if busy else "idle", activity="busy" if busy else "idle",
                                activity_summary="Bash: ./scripts/smoke-test.sh" if busy else "",
                                read=dict(flip["read"], reply_id="" if not busy and n % 6 == 3 else flip["reply_id"])))
        steps.append({"upsert": upserts, "remove": []})
    if len(idle) > 1:
        old = idle[1]
        new = dict(old, run_id=old["run_id"] + "-2", conversation_id=old["conversation_id"] + "-2")
        steps[4]["remove"].append({"key": key_of(old), "reason": "replaced"})
        steps[4]["upsert"].append(new)
        steps[10]["remove"].append({"key": key_of(new), "reason": "replaced"})
        steps[10]["upsert"].append(old)
    return steps


def stream(host, rows, long_key):
    frames = []
    seq = [0]

    def frame(t, **body):
        seq[0] += 1
        frames.append({"v": 1, "seq": seq[0], "t": t, **body})

    frame("hello", machine_id="00000000-0000-4000-8000-0000000000" + ("01" if host == "workstation" else "02"), host=host, hgs_version="1.56.0",
          stream=f"stream-{host}", heartbeat_ms=10000, max_frame_bytes=1 << 20, topics=["catalog", "catalog.meta", "activity", "git"],
          limits={"activity_subscriptions": 16, "page_bytes": 524288, "queue_bytes": 4194304, "activity_tail": 100, "activity_read": 200,
                  "control_line_bytes": 65536, "git_subscriptions": 64})
    half = len(rows) // 2
    pages = [rows[:half], rows[half:]]
    for page, chunk in enumerate(pages):
        frame("snapshot", topic="catalog", items=[{"key": key_of(r), "row": r} for r in chunk], page=page, last=page == len(pages) - 1, cursor=1)
    frame("snapshot", topic="catalog.meta", value={"host": host, "ok": True, "projects": {}, "peers": []}, page=0, last=True, cursor=1)
    activity = {}
    for r in rows:
        count = 1100 if key_of(r) == long_key else rng.randint(8, 40)
        events = events_for(r, count)
        activity[key_of(r)] = (events, state_for(r))
    # Activity first, so the replay holds every session's log before the deltas loop.
    for key, (events, state) in activity.items():
        for start in range(0, len(events), 200):
            chunk = events[start:start + 200]
            frame("events", topic="activity", key=key, events=chunk, cursor=chunk[-1]["seq"])
        frame("state", topic="activity", key=key, value=state, rev=1)
    for n, step in enumerate(deltas_for(rows)):
        frame("heartbeat", at=BASE_AT + n)
        frame("delta", topic="catalog", upsert=[{"key": key_of(r), "row": r} for r in step["upsert"]], remove=step["remove"], cursor=2 + n)
    return frames, activity


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    local_rows = session_rows(PROJECTS, 0)
    # The long Activity (1 100 events) belongs to the first working session.
    long_key = key_of(next(r for r in local_rows if r["activity"] == "busy"))
    mac_rows = session_rows(MAC_PROJECTS, 7)
    computers = [("", "workstation", local_rows, long_key), ("mac", "mac", mac_rows, None)]
    batches = {"interval_ms": INTERVAL_MS, "computers": [], "snapshot": [], "deltas": [], "activity": {}}
    for computer, host, rows, long in computers:
        frames, activity = stream(host, rows, long)
        name = "replay.ndjson" if computer == "" else f"replay-{computer}.ndjson"
        (ROOT / name).write_text("".join(json.dumps(f, separators=(",", ":")) + "\n" for f in frames), encoding="utf-8")
        machine = host
        batches["computers"].append({"computer": computer, "status": "live", "attempt": 0})
        batches["snapshot"].append({"computer": computer, "rows": [{"computer": computer, "key": key_of(r), "row": view_row(r, machine)} for r in rows]})
        for n, step in enumerate(deltas_for(rows)):
            if len(batches["deltas"]) <= n:
                batches["deltas"].append({"upserts": [], "removes": []})
            batches["deltas"][n]["upserts"] += [{"computer": computer, "key": key_of(r), "row": view_row(r, machine)} for r in step["upsert"]]
            batches["deltas"][n]["removes"] += [{"computer": computer, "key": k["key"], "reason": k["reason"]} for k in step["remove"]]
        for key, (events, state) in activity.items():
            batches["activity"][f"{computer}\n{key}"] = {"events": events, "cursor": events[-1]["seq"], "state": state}
    (ROOT / "replay-batches.json").write_text(json.dumps(batches, separators=(",", ":")) + "\n", encoding="utf-8")
    for path in sorted(ROOT.iterdir()):
        print(f"{path.name}: {path.stat().st_size} bytes")


if __name__ == "__main__":
    main()
