// Presentation-layer types only. The provider core (SQLite canonical
// conversation, Context Manager, Session Manager, CLI adapters) owns the real
// shapes; everything here is what the UI needs in order to render them.

export type ProviderId = "claude" | "codex" | "gemini";

export type EffortLevel = "low" | "medium" | "high" | "xhigh";

export type ConnectionStatus =
  | "connected"
  | "signin-required"
  | "not-installed"
  | "disconnected"
  | "error"
  | "checking";

export interface ModelInfo {
  id: string;
  label: string;
  /** null when the CLI does not report a context window for this model. */
  contextWindow: number | null;
}

export interface AccountProfile {
  id: string;
  /** User-editable label — "Work", "Personal", "Main". Never a raw account id. */
  label: string;
  status: ConnectionStatus;
  models: ModelInfo[];
}

export interface ProviderCapabilities {
  /**
   * Reasoning-effort levels this provider exposes, in order.
   * null means the CLI reports no reasoning control — the UI hides the
   * Effort selector entirely rather than showing a dead one.
   */
  effort: EffortLevel[] | null;
  /** Whether the CLI reports plan/quota usage at all. */
  usageLimits: boolean;
  /** Whether the CLI reports a context-window size for the active session. */
  contextWindow: boolean;
}

export interface ProviderInfo {
  id: ProviderId;
  label: string;
  color: string;
  cliVersion: string | null;
  status: ConnectionStatus;
  capabilities: ProviderCapabilities;
  accounts: AccountProfile[];
}

export interface Selection {
  provider: ProviderId;
  accountId: string;
  modelId: string;
  /** null when the selected provider exposes no reasoning control. */
  effort: EffortLevel | null;
}

/** A usage window as reported by a CLI. null fields mean "not reported". */
export interface UsageWindow {
  label: string;
  percent: number | null;
  resetsAt: string | null;
}

export interface AccountUsage {
  providerId: ProviderId;
  accountId: string;
  /** null means the CLI exposes no plan usage for this account. */
  windows: UsageWindow[] | null;
}

/** Context usage for the ACTIVE model/session only — never a plan quota. */
export interface ContextUsage {
  used: number;
  window: number | null;
  input: number | null;
  output: number | null;
  reasoning: number | null;
  cacheRead: number | null;
  updatedAt?: string | null;
}

export interface ThinkingData {
  content: string;
  wordCount: number;
}

export interface ToolCallData {
  id: string;
  name: string;
  args: Record<string, string>;
  result?: string;
  durationMs?: number;
  status: "running" | "completed" | "error";
}

export interface ApprovalData {
  tool: string;
  command: string;
  target: string;
  note: string;
}

export interface AttachmentFile {
  id: string;
  name: string;
  size: string;
  kind: "file" | "image";
}

export interface Message {
  id: string;
  seq: number;
  role: "user" | "assistant";
  content: string;
  timestamp: string;
  /** Which provider/account/model produced this turn. Absent on user turns. */
  origin?: {
    provider: ProviderId;
    accountLabel: string;
    modelLabel: string;
    effort: EffortLevel | null;
  };
  thinking?: ThinkingData;
  toolCalls?: ToolCallData[];
  approval?: ApprovalData;
  attachments?: AttachmentFile[];
}

export interface Conversation {
  id: string;
  title: string;
  lastProvider: ProviderId;
  ago: string;
  pinned?: boolean;
  groupId?: string;
}

export interface ConversationGroup {
  id: string;
  label: string;
  collapsed: boolean;
}
