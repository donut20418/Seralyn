import { useState } from "react";
import { Check, Copy, X } from "lucide-react";
import { PROVIDER_ORDER, PROVIDERS } from "../data/providers";
import { NATIVE_SESSIONS, SYNC_CURSORS } from "../data/mock";
import type { Message, ProviderId, ProviderInfo } from "../types";

function CopyRow({ label, value, color }: { label: string; value: string | null; color: string }) {
  const [copied, setCopied] = useState(false);

  return (
    <div className="sr-insp-row">
      <span className="sr-dot" style={{ background: color }} />
      <span style={{ color: "var(--sr-text-2)", width: 62, flexShrink: 0 }}>{label}</span>
      <span className="sr-insp-row__val" style={{ flexGrow: 1 }}>
        {value ?? "Unavailable"}
      </span>
      {value && (
        <button
          className="sr-icon-btn"
          style={{ width: 20, height: 20 }}
          aria-label={`Copy ${label} session id`}
          onClick={() => {
            void navigator.clipboard?.writeText(value);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1600);
          }}
        >
          {copied ? <Check size={11} color="var(--sr-ok)" /> : <Copy size={11} />}
        </button>
      )}
    </div>
  );
}

interface Props {
  messages: Message[];
  onClose: () => void;
  nativeSessions?: Record<ProviderId, string | null>;
  syncCursors?: Record<ProviderId, { synced: number; current: number }>;
  providers?: Record<ProviderId, ProviderInfo>;
}

/**
 * The deep, technical view. Deliberately separate from the composer's quick
 * context popup so that popup never grows into this.
 */
export function Inspector({
  messages,
  onClose,
  nativeSessions = NATIVE_SESSIONS,
  syncCursors = SYNC_CURSORS,
  providers = PROVIDERS,
}: Props) {
  const maxSeq = messages.reduce((n, m) => Math.max(n, m.seq), 0);

  return (
    <div className="sr-inspector">
      <div className="sr-inspector__head">
        <span style={{ fontSize: 13, fontWeight: 500 }}>Context Inspector</span>
        <span className="sr-spacer" />
        <button className="sr-icon-btn" aria-label="Close inspector" onClick={onClose}>
          <X size={14} />
        </button>
      </div>

      <div className="sr-inspector__body sr-scroll">
        <section className="sr-insp-section">
          <div className="sr-label" style={{ marginBottom: 6 }}>
            Native sessions
          </div>
          {PROVIDER_ORDER.map((id: ProviderId) => (
            <CopyRow
              key={id}
              label={providers[id]?.label ?? PROVIDERS[id].label}
              value={nativeSessions[id]}
              color={providers[id]?.color ?? PROVIDERS[id].color}
            />
          ))}
        </section>

        <section className="sr-insp-section">
          <div className="sr-label" style={{ marginBottom: 6 }}>
            Sync cursor
          </div>
          {PROVIDER_ORDER.map((id: ProviderId) => {
            const cursor = syncCursors[id] ?? { synced: 0, current: maxSeq };
            const behind = cursor.current - cursor.synced;
            return (
              <div className="sr-insp-row" key={id}>
                <span className="sr-dot" style={{ background: providers[id]?.color ?? PROVIDERS[id].color }} />
                <span style={{ color: "var(--sr-text-2)", width: 62, flexShrink: 0 }}>
                  {providers[id]?.label ?? PROVIDERS[id].label}
                </span>
                <span className="sr-insp-row__val" style={{ flexGrow: 1 }}>
                  synced #{cursor.synced} of #{cursor.current}
                </span>
                <span
                  style={{
                    fontSize: 11,
                    color: behind ? "var(--sr-warn)" : "var(--sr-text-3)",
                  }}
                >
                  {behind ? `${behind} to replay` : "up to date"}
                </span>
              </div>
            );
          })}
          <p style={{ margin: "8px 0 0", fontSize: 11, lineHeight: 1.6, color: "var(--sr-text-3)" }}>
            Turns a provider has not seen are replayed on the next switch to it. The canonical
            conversation is unaffected.
          </p>
        </section>

        <section className="sr-insp-section">
          <div className="sr-label" style={{ marginBottom: 6 }}>
            Turn tree — {maxSeq} turns
          </div>
          <div className="sr-turn-tree">
            {messages.map((m) => (
              <div className="sr-turn-tree__row" key={m.id}>
                <span className="sr-turn-tree__seq">#{m.seq}</span>
                {m.origin ? (
                  <>
                    <span className="sr-dot" style={{ background: (providers[m.origin.provider] ?? PROVIDERS[m.origin.provider])?.color }} />
                    <span style={{ color: (providers[m.origin.provider] ?? PROVIDERS[m.origin.provider])?.color }}>
                      {(providers[m.origin.provider] ?? PROVIDERS[m.origin.provider])?.label}
                    </span>
                    <span style={{ color: "var(--sr-text-3)", fontSize: 11 }}>
                      {m.origin.modelLabel}
                    </span>
                  </>
                ) : (
                  <>
                    <span className="sr-dot sr-dot--hollow" />
                    <span>User</span>
                  </>
                )}
                <span className="sr-spacer" />
                <span className="sr-turn__time">{m.timestamp}</span>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
