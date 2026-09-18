import type {
  AccountUsage,
  Conversation,
  ConversationGroup,
  ContextUsage,
  Message,
  ProviderId,
} from "../types";

/**
 * Placeholder data standing in for the provider core while the UI is developed.
 * Replace each export with the real query — the shapes are what the components
 * consume. Nothing here is computed or estimated by the UI.
 */

export const GROUPS: ConversationGroup[] = [
  { id: "work", label: "Work", collapsed: false },
  { id: "personal", label: "Personal", collapsed: true },
];

export const CONVERSATIONS: Conversation[] = [
  { id: "p1", title: "Maya Rig Tool", lastProvider: "claude", ago: "4h", pinned: true },
  { id: "p2", title: "Seralyn Architecture", lastProvider: "codex", ago: "1d", pinned: true },
  { id: "g1", title: "Pipeline Tools", lastProvider: "codex", ago: "20m", groupId: "work" },
  { id: "g2", title: "Shader Research", lastProvider: "claude", ago: "3h", groupId: "work" },
  { id: "g3", title: "Maya Automation", lastProvider: "gemini", ago: "2d", groupId: "work" },
  { id: "g4", title: "AI Research", lastProvider: "gemini", ago: "5d", groupId: "personal" },
  { id: "g5", title: "Notes", lastProvider: "claude", ago: "1w", groupId: "personal" },
  { id: "r1", title: "Async file watcher in Rust", lastProvider: "claude", ago: "2h" },
  { id: "r2", title: "Tauri IPC architecture", lastProvider: "codex", ago: "1d" },
  { id: "r3", title: "Optimize Postgres queries", lastProvider: "gemini", ago: "3d" },
  { id: "r4", title: "API rate limiting", lastProvider: "claude", ago: "1w" },
];

export const MESSAGES: Message[] = [
  {
    id: "m1",
    seq: 1,
    role: "user",
    timestamp: "14:02",
    content:
      "The rebuild loop fires twice on every save in VS Code. Can you debounce the watcher and keep the cancellation path intact?",
    attachments: [{ id: "a1", name: "watcher.rs", size: "2.4 KB", kind: "file" }],
  },
  {
    id: "m2",
    seq: 2,
    role: "assistant",
    timestamp: "14:02",
    origin: { provider: "claude", accountLabel: "Work", modelLabel: "Sonnet 5", effort: "high" },
    thinking: {
      wordCount: 1240,
      content:
        "Two events per save is the editor writing twice — a truncate followed by a write. The fix is a debounce window wide enough to swallow both but short enough to stay responsive; 300 ms covers every editor I know of. The cancellation token has to be re-created at the top of the loop, otherwise the second batch cancels its own build.",
    },
    content:
      "The double rebuild comes from the editor writing twice on save — a truncate, then a write. A 300 ms debounce collapses both into one batch, and re-creating the cancellation token at the top of the loop aborts the in-flight build instead of queueing behind it.\n\n```rust\nlet mut debouncer = new_debouncer(Duration::from_millis(300), tx)?;\ndebouncer.watcher().watch(root, RecursiveMode::Recursive)?;\n\nwhile let Some(batch) = rx.recv().await {\n    if batch.iter().any(is_rust_source) {\n        cancel.cancel();\n        cancel = CancellationToken::new();\n        spawn_build(cancel.clone());\n    }\n}\n```\n\nTune the window to your editor if 300 ms feels sluggish — anything above the write gap works.",
  },
  {
    id: "m3",
    seq: 3,
    role: "user",
    timestamp: "14:08",
    content: "Run it against the workspace and show me what actually fires on a save.",
  },
  {
    id: "m4",
    seq: 4,
    role: "assistant",
    timestamp: "14:09",
    origin: { provider: "codex", accountLabel: "Main", modelLabel: "GPT-5.1-Codex", effort: "medium" },
    toolCalls: [
      {
        id: "t1",
        name: "Run command",
        args: { command: "cargo run -p watchd -- --once", cwd: "~/seralyn-core" },
        result:
          "14:22:41  event   Modify(Data) src/watcher.rs\n14:22:41  event   Modify(Data) src/watcher.rs\n14:22:41  batch   2 events -> 1 rebuild\n14:22:41  cancel  previous build at 14 ms\n14:22:42  built   watchd in 0.94 s",
        durationMs: 1400,
        status: "completed",
      },
    ],
    content:
      "One `Modify(Data)` event per save now — the second write lands inside the debounce window:\n\n- Editor save emits 2 raw events, 11 ms apart.\n- The debouncer forwards 1 batch at t+300 ms.\n- Previous build cancels 14 ms in; nothing queues.\n\n> Median rebuild is 0.94 s across 14 runs, with 3 cancellations and no duplicates.",
  },
  {
    id: "m5",
    seq: 5,
    role: "user",
    timestamp: "14:15",
    content: "Add structured logging with `tracing` so the cancel path is visible in the log.",
  },
  {
    id: "m6",
    seq: 6,
    role: "assistant",
    timestamp: "14:15",
    origin: { provider: "gemini", accountLabel: "Studio", modelLabel: "Gemini 3 Pro", effort: null },
    approval: {
      tool: "Terminal",
      command: "cargo add",
      target: "tracing tracing-subscriber tokio-util",
      note: "Modifies Cargo.toml and downloads crates from crates.io.",
    },
    content:
      "I need the tracing dependencies before I can instrument the cancel path. Approve the command and I'll wire up a span per rebuild with the cancellation reason attached.",
  },
];

/** Context usage for the ACTIVE session only. */
export const CONTEXT_USAGE: ContextUsage = {
  used: 44_220,
  window: 200_000,
  input: 28_640,
  output: 9_120,
  reasoning: 2_180,
  cacheRead: 4_280,
};

/**
 * Plan usage per linked account. `windows: null` means the CLI exposes no
 * quota information — the UI prints "Unavailable" rather than a number.
 */
export const ACCOUNT_USAGE: AccountUsage[] = [
  {
    providerId: "claude",
    accountId: "claude-work",
    windows: [
      { label: "5-hour", percent: 56, resetsAt: "in 3h 10m" },
      { label: "Weekly", percent: 69, resetsAt: "Tue 7:00 PM" },
    ],
  },
  {
    providerId: "claude",
    accountId: "claude-personal",
    windows: [
      { label: "5-hour", percent: 21, resetsAt: "in 4h 02m" },
      { label: "Weekly", percent: 33, resetsAt: "Tue 7:00 PM" },
    ],
  },
  {
    providerId: "codex",
    accountId: "codex-main",
    windows: [
      // This CLI reports percentages but no reset timestamps.
      { label: "5-hour", percent: 48, resetsAt: null },
      { label: "Weekly", percent: 61, resetsAt: null },
    ],
  },
  {
    providerId: "gemini",
    accountId: "gemini-studio",
    windows: null,
  },
];

export const NATIVE_SESSIONS: Record<ProviderId, string | null> = {
  claude: "sess_01XKqmR8vNpL2dTn",
  codex: "thread_abc123def456gh7",
  gemini: null,
};

export const SYNC_CURSORS: Record<ProviderId, { synced: number; current: number }> = {
  claude: { synced: 6, current: 6 },
  codex: { synced: 4, current: 6 },
  gemini: { synced: 6, current: 6 },
};
