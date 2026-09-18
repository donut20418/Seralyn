import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type DragEvent,
  type KeyboardEvent,
} from "react";
import { ChevronDown, FileText, Globe, ImageIcon, Plus, Square, Upload, X, Zap } from "lucide-react";
import { ContextPopover, contextPercent, usageColor } from "./ContextPopover";
import { EffortMenu } from "./EffortMenu";
import { ProviderSelector } from "./ProviderSelector";
import { EFFORT_LABEL, effortOptions, getAccount, getModel } from "../data/providers";
import type {
  AccountUsage,
  AttachmentFile,
  ContextUsage,
  EffortLevel,
  ProviderId,
  ProviderInfo,
  Selection,
} from "../types";

type Popover = "model" | "effort" | "context" | null;

/** Donut showing how full the active model's context window is. */
function ContextRing({ percent }: { percent: number }) {
  const r = 5.5;
  const c = 2 * Math.PI * r;
  return (
    <svg width={13} height={13} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r={r} stroke="rgba(255,255,255,0.16)" strokeWidth="2" />
      <circle
        cx="8"
        cy="8"
        r={r}
        stroke={usageColor(percent)}
        strokeWidth="2"
        strokeLinecap="round"
        strokeDasharray={`${(c * percent) / 100} ${c}`}
        transform="rotate(-90 8 8)"
      />
    </svg>
  );
}

interface Props {
  providers: Record<ProviderId, ProviderInfo>;
  selection: Selection;
  onSelect: (provider: ProviderId, accountId: string, modelId: string) => void;
  onSetEffort: (level: EffortLevel) => void;
  onRenameAccount: (provider: ProviderId, accountId: string, label: string) => void;
  contextUsage: ContextUsage;
  accountUsage: AccountUsage[];
  generating: boolean;
  onSend: (text: string, attachments: AttachmentFile[]) => void;
  onStop: () => void;
  webOpen: boolean;
  onToggleWeb: () => void;
  onManageProviders: () => void;
  onViewUsage: () => void;
  onUploadFile?: (file: File) => Promise<AttachmentFile>;
  onRemoveAttachment?: (id: string) => void;
  syncedThroughTurn?: number;
}

export function Composer({
  providers,
  selection,
  onSelect,
  onSetEffort,
  onRenameAccount,
  contextUsage,
  accountUsage,
  generating,
  onSend,
  onStop,
  webOpen,
  onToggleWeb,
  onManageProviders,
  onViewUsage,
  onUploadFile,
  onRemoveAttachment,
  syncedThroughTurn,
}: Props) {
  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<AttachmentFile[]>([]);
  const [popover, setPopover] = useState<Popover>(null);
  const [focused, setFocused] = useState(false);
  const [dropping, setDropping] = useState(false);

  const wrapRef = useRef<HTMLDivElement>(null);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const dragDepth = useRef(0);

  const provider = providers[selection.provider] ?? { label: selection.provider, color: "var(--sr-accent)" };
  const account = getAccount(selection.provider, selection.accountId, providers);
  const model = getModel(selection.provider, selection.accountId, selection.modelId, providers);
  const efforts = effortOptions(selection.provider, providers);
  const pct = contextPercent(contextUsage);

  // Close the open popover on Escape or a click outside the composer column.
  useEffect(() => {
    if (!popover) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") setPopover(null);
    };
    const onDown = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) setPopover(null);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onDown);
    };
  }, [popover]);

  // Grow the input with its content, up to the CSS max-height.
  useEffect(() => {
    const el = textRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [draft]);

  const toggle = (next: Exclude<Popover, null>) =>
    setPopover((current) => (current === next ? null : next));

  const addFiles = async (files: FileList | null) => {
    if (!files?.length) return;
    for (const f of Array.from(files)) {
      if (onUploadFile) {
        try {
          const uploaded = await onUploadFile(f);
          setAttachments((prev) => [...prev, uploaded]);
        } catch (e) {
          console.error("Attachment upload error:", e);
        }
      } else {
        const next: AttachmentFile = {
          id: `${Date.now()}-${Math.random().toString(36).substring(2, 7)}`,
          name: f.name,
          size: f.size > 1024 * 1024 ? `${(f.size / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(f.size / 1024))} KB`,
          kind: f.type.startsWith("image/") ? "image" : "file",
        };
        setAttachments((prev) => [...prev, next]);
      }
    }
  };

  const send = () => {
    if (!draft.trim() && attachments.length === 0) return;
    onSend(draft.trim(), attachments);
    setDraft("");
    setAttachments([]);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (!generating) send();
    }
  };

  const onDragEnter = (e: DragEvent) => {
    e.preventDefault();
    dragDepth.current += 1;
    setDropping(true);
  };
  const onDragLeave = (e: DragEvent) => {
    e.preventDefault();
    dragDepth.current -= 1;
    if (dragDepth.current <= 0) setDropping(false);
  };
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    dragDepth.current = 0;
    setDropping(false);
    addFiles(e.dataTransfer.files);
  };

  const canSend = draft.trim().length > 0 || attachments.length > 0;

  return (
    <div className="sr-composer-wrap">
      <div className="sr-composer-col" ref={wrapRef}>
        <div className="sr-pop-layer">
          {popover === "model" && (
            <ProviderSelector
              providers={providers}
              selection={selection}
              onSelect={(p, a, m) => {
                onSelect(p, a, m);
                setPopover(null);
              }}
              onRenameAccount={onRenameAccount}
              onManageProviders={() => {
                setPopover(null);
                onManageProviders();
              }}
            />
          )}

          {popover === "effort" && efforts && (
            <EffortMenu
              options={efforts}
              value={selection.effort}
              providerLabel={provider.label}
              modelLabel={model?.label ?? "this model"}
              onPick={(level) => {
                onSetEffort(level);
                setPopover(null);
              }}
            />
          )}

          {popover === "context" && (
            <ContextPopover
              usage={contextUsage}
              providerLabel={provider.label}
              accountLabel={account?.label ?? ""}
              modelLabel={model?.label ?? ""}
              accountUsage={accountUsage}
              providers={providers}
              onViewDetails={() => {
                setPopover(null);
                onViewUsage();
              }}
            />
          )}
        </div>

        <div
          className={[
            "sr-composer",
            focused && !dropping ? "sr-composer--focus" : "",
            dropping ? "sr-composer--drop" : "",
          ]
            .filter(Boolean)
            .join(" ")}
          onDragEnter={onDragEnter}
          onDragOver={(e) => e.preventDefault()}
          onDragLeave={onDragLeave}
          onDrop={onDrop}
          style={{ position: "relative" }}
        >
          {attachments.length > 0 && (
            <div className="sr-composer__atts">
              {attachments.map((att) => (
                <span className="sr-att" key={att.id}>
                  {att.kind === "image" ? <ImageIcon size={11} /> : <FileText size={11} />}
                  <span className="sr-att__name">{att.name}</span>
                  <span className="sr-att__size">{att.size}</span>
                  <button
                    className="sr-att__x"
                    aria-label={`Remove ${att.name}`}
                    onClick={() => {
                      setAttachments((prev) => prev.filter((a) => a.id !== att.id));
                      onRemoveAttachment?.(att.id);
                    }}
                  >
                    <X size={10} />
                  </button>
                </span>
              ))}
            </div>
          )}

          <textarea
            ref={textRef}
            className="sr-composer__input"
            rows={2}
            placeholder="Ask anything, or paste a stack trace…"
            value={draft}
            onChange={(e: ChangeEvent<HTMLTextAreaElement>) => setDraft(e.target.value)}
            onKeyDown={onKeyDown}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            aria-label="Message"
          />

          <div className="sr-composer__controls">
            <input
              ref={fileRef}
              type="file"
              multiple
              className="sr-sr-only"
              onChange={(e) => {
                addFiles(e.target.files);
                e.target.value = "";
              }}
            />
            <button
              className="sr-icon-btn"
              style={{ width: 28, height: 28 }}
              aria-label="Attach file"
              onClick={() => fileRef.current?.click()}
            >
              <Plus size={15} />
            </button>

            <button
              className="sr-chip"
              onClick={() => toggle("model")}
              aria-haspopup="dialog"
              aria-expanded={popover === "model"}
            >
              <span className="sr-dot" style={{ background: provider.color }} />
              <span>
                {provider.label} · {account?.label}
              </span>
              <span className="sr-chip__strong">{model?.label}</span>
              <ChevronDown size={11} className="sr-chip__caret" />
            </button>

            {/* Rendered only when the provider actually exposes reasoning control. */}
            {efforts && selection.effort && (
              <button
                className="sr-chip"
                onClick={() => toggle("effort")}
                aria-haspopup="dialog"
                aria-expanded={popover === "effort"}
              >
                <Zap size={12} />
                <span style={{ color: "var(--sr-text)" }}>{EFFORT_LABEL[selection.effort]}</span>
                <ChevronDown size={11} className="sr-chip__caret" />
              </button>
            )}

            <button
              className="sr-chip"
              onClick={() => toggle("context")}
              aria-haspopup="dialog"
              aria-expanded={popover === "context"}
              aria-label="Context and usage"
            >
              {pct === null ? <FileText size={12} /> : <ContextRing percent={pct} />}
              <span className="sr-mono" style={{ fontSize: 11.5, color: "var(--sr-text)" }}>
                {pct === null ? "—" : `${pct}%`}
              </span>
            </button>

            <button
              className={`sr-chip${webOpen ? " sr-chip--on" : ""}`}
              onClick={onToggleWeb}
              aria-pressed={webOpen}
            >
              <Globe size={13} />
              <span style={{ color: webOpen ? "var(--sr-accent)" : "var(--sr-text)" }}>Web</span>
            </button>

            <span className="sr-spacer" />

            {/* Same slot, same size — only the label and colour change. */}
            {generating ? (
              <button className="sr-btn sr-btn--danger" onClick={onStop}>
                Stop
                <Square size={12} fill="currentColor" />
              </button>
            ) : (
              <button
                className={`sr-btn${canSend ? " sr-btn--accent" : ""}`}
                onClick={send}
                disabled={!canSend}
              >
                Send
                <svg width={12} height={12} viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <path
                    d="M8 13.2V3.2M4 7.2l4-4 4 4"
                    stroke="currentColor"
                    strokeWidth="1.6"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                </svg>
              </button>
            )}
          </div>

          {dropping && (
            <div className="sr-drop">
              <Upload size={15} />
              Drop files to attach
            </div>
          )}
        </div>

        <div className="sr-composer__hint">
          {generating ? (
            <span>
              <span className="sr-mono">Esc</span> stop
            </span>
          ) : (
            <>
              <span>
                <span className="sr-mono">⏎</span> send
              </span>
              <span>
                <span className="sr-mono">⇧⏎</span> newline
              </span>
            </>
          )}
          <span className="sr-spacer" />
          <span>Canonical conversation synced through turn #{syncedThroughTurn ?? 0}</span>
        </div>
      </div>
    </div>
  );
}
