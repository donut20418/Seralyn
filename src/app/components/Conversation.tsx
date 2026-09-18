import { useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  FileText,
  ImageIcon,
  Loader2,
  Sparkles,
} from "lucide-react";
import { Markdown } from "./Markdown";
import { EFFORT_LABEL, PROVIDERS } from "../data/providers";
import type { ApprovalData, Message, ThinkingData, ToolCallData } from "../types";

/* -------------------------------------------------------------- thinking --- */

function Thinking({ data, live }: { data: ThinkingData; live: boolean }) {
  const [open, setOpen] = useState(false);

  if (live) {
    return (
      <div className="sr-thinking" aria-live="polite">
        <Sparkles size={12} />
        <span>Thinking</span>
        <span className="sr-pulse" aria-hidden="true">
          <span />
          <span />
          <span />
        </span>
      </div>
    );
  }

  return (
    <>
      <button className="sr-thinking" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
        <span>Thinking</span>
        {!open && <span className="sr-thinking__count">{data.wordCount.toLocaleString()} words</span>}
      </button>
      {open && <p className="sr-thinking__body">{data.content}</p>}
    </>
  );
}

/* ------------------------------------------------------------- tool card --- */

function ToolCard({ tool }: { tool: ToolCallData }) {
  const [open, setOpen] = useState(false);

  const color =
    tool.status === "completed"
      ? "var(--sr-ok)"
      : tool.status === "error"
        ? "var(--sr-danger)"
        : "var(--sr-accent)";
  const label =
    tool.status === "completed" ? "Completed" : tool.status === "error" ? "Error" : "Running";
  const firstArg = Object.values(tool.args)[0];

  return (
    <div className="sr-tool">
      <button className="sr-tool__head" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        {tool.status === "running" ? (
          <Loader2 size={12} color={color} className="sr-spin" />
        ) : tool.status === "error" ? (
          <AlertTriangle size={12} color={color} />
        ) : (
          <Check size={12} color={color} />
        )}
        <span className="sr-tool__state" style={{ color }}>
          {label}
        </span>
        <span className="sr-tool__name">{tool.name}</span>
        <span className="sr-tool__arg">{firstArg}</span>
        <span className="sr-spacer" />
        {tool.durationMs !== undefined && (
          <span className="sr-tool__dur">{(tool.durationMs / 1000).toFixed(1)}s</span>
        )}
        {open ? <ChevronDown size={11} color="var(--sr-text-3)" /> : <ChevronRight size={11} color="var(--sr-text-3)" />}
      </button>

      {open && (
        <div className="sr-tool__body">
          <div className="sr-label" style={{ marginBottom: 4 }}>
            Arguments
          </div>
          {Object.entries(tool.args).map(([k, v]) => (
            <div className="sr-tool__row" key={k}>
              <span className="sr-tool__key">{k}</span>
              <span className="sr-tool__val">{v}</span>
            </div>
          ))}
          {tool.result && (
            <>
              <div className="sr-label" style={{ marginTop: 10 }}>
                Output
              </div>
              <pre className="sr-tool__out sr-scroll">{tool.result}</pre>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/* -------------------------------------------------------------- approval --- */

function Approval({
  data,
  onRespond,
}: {
  data: ApprovalData;
  onRespond?: (approved: boolean) => void;
}) {
  const [decision, setDecision] = useState<"approved" | "denied" | null>(null);

  const handleDecision = (approved: boolean) => {
    setDecision(approved ? "approved" : "denied");
    onRespond?.(approved);
  };

  if (decision) {
    return (
      <p className="sr-approval__done">
        {decision === "approved" ? "Approved — command ran." : "Denied — nothing was run."}
      </p>
    );
  }

  return (
    <div className="sr-approval">
      <div className="sr-approval__head">
        <AlertTriangle size={13} />
        Permission required
        <span className="sr-spacer" />
        <span style={{ color: "var(--sr-text-3)", fontWeight: 400 }}>{data.tool}</span>
      </div>
      <div className="sr-approval__row">
        <span className="sr-approval__key">Command</span>
        <span className="sr-mono" style={{ color: "var(--sr-text-2)" }}>
          {data.command}
        </span>
      </div>
      <div className="sr-approval__row">
        <span className="sr-approval__key">Target</span>
        <span className="sr-mono" style={{ color: "var(--sr-text-2)" }}>
          {data.target}
        </span>
      </div>
      <div className="sr-approval__row">
        <span className="sr-approval__key">Note</span>
        <span style={{ color: "var(--sr-text-2)" }}>{data.note}</span>
      </div>
      <div className="sr-approval__actions">
        <button className="sr-btn" onClick={() => handleDecision(false)}>
          Deny
        </button>
        <button className="sr-btn sr-btn--accent" onClick={() => handleDecision(true)}>
          Approve
        </button>
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- turn ----- */

function Turn({
  message,
  live,
  onRespondApproval,
}: {
  message: Message;
  live: boolean;
  onRespondApproval?: (approved: boolean) => void;
}) {
  if (message.role === "user") {
    return (
      <div>
        <div className="sr-turn__head">
          <span className="sr-turn__who" style={{ color: "var(--sr-text-2)" }}>
            You
          </span>
          <span className="sr-spacer" />
          <span className="sr-turn__time">{message.timestamp}</span>
        </div>
        <div className="sr-user-bubble">
          <p>{message.content}</p>
          {message.attachments && message.attachments.length > 0 && (
            <div className="sr-attachments">
              {message.attachments.map((a) => (
                <span className="sr-att" key={a.id}>
                  {a.kind === "image" ? <ImageIcon size={11} /> : <FileText size={11} />}
                  <span className="sr-att__name">{a.name}</span>
                  <span className="sr-att__size">{a.size}</span>
                </span>
              ))}
            </div>
          )}
        </div>
      </div>
    );
  }

  const origin = message.origin;
  const provider = origin ? PROVIDERS[origin.provider] : null;

  return (
    <div>
      {origin && provider && (
        <div className="sr-turn__head">
          <span className="sr-dot" style={{ background: provider.color }} />
          <span className="sr-turn__who" style={{ color: provider.color }}>
            {provider.label}
          </span>
          <span className="sr-turn__meta">
            {[origin.accountLabel, origin.modelLabel, origin.effort ? EFFORT_LABEL[origin.effort] : null]
              .filter(Boolean)
              .join(" · ")}
          </span>
          <span className="sr-spacer" />
          <span className="sr-turn__time">{message.timestamp}</span>
        </div>
      )}

      {message.thinking && <Thinking data={message.thinking} live={live} />}
      {message.toolCalls?.map((t) => <ToolCard key={t.id} tool={t} />)}

      <Markdown content={message.content} />

      {message.approval && <Approval data={message.approval} onRespond={onRespondApproval} />}
    </div>
  );
}

/* ------------------------------------------------------------------ list --- */

export function Conversation({
  messages,
  generating,
  onRespondApproval,
}: {
  messages: Message[];
  generating: boolean;
  onRespondApproval?: (approved: boolean) => void;
}) {
  const endRef = useRef<HTMLDivElement>(null);

  // Follow the conversation as turns arrive, the way a terminal follows output.
  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [messages.length, generating]);

  return (
    <div className="sr-convo sr-scroll">
      <div className="sr-convo__inner">
        {messages.map((m, i) => (
          <Turn
            key={m.id}
            message={m}
            live={generating && i === messages.length - 1}
            onRespondApproval={onRespondApproval}
          />
        ))}
        <div ref={endRef} />
      </div>
    </div>
  );
}
