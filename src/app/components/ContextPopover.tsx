import { Info } from "lucide-react";
import { formatTokens } from "../data/providers";
import type { AccountUsage, ContextUsage, ProviderId, ProviderInfo } from "../types";

export function usageColor(pct: number) {
  if (pct < 60) return "var(--sr-ok)";
  if (pct < 80) return "var(--sr-warn)";
  if (pct < 90) return "#f08a4b";
  return "var(--sr-danger)";
}

/** Percentage of the context window in use, or null when the window is unknown. */
export function contextPercent(usage: ContextUsage): number | null {
  if (usage.window === null || usage.window <= 0) return null;
  return Math.min(100, Math.round((usage.used / usage.window) * 100));
}

function formatUpdatedAgo(dateStr?: string | null): string {
  if (!dateStr) return "recently";
  const d = new Date(dateStr);
  if (isNaN(d.getTime())) return "recently";
  const diffSecs = Math.max(0, Math.floor((Date.now() - d.getTime()) / 1000));
  if (diffSecs < 10) return "just now";
  if (diffSecs < 60) return `${diffSecs}s ago`;
  const diffMins = Math.floor(diffSecs / 60);
  if (diffMins < 60) return `${diffMins}m ago`;
  return `${Math.floor(diffMins / 60)}h ago`;
}

function Row({ label, value }: { label: string; value: number | null }) {
  return (
    <div className="sr-ctx__row">
      <span>{label}</span>
      <span>{value === null ? "Unavailable" : value.toLocaleString()}</span>
    </div>
  );
}

interface Props {
  usage: ContextUsage;
  modelLabel: string;
  accountLabel: string;
  providerLabel: string;
  accountUsage: AccountUsage[];
  providers: Record<ProviderId, ProviderInfo>;
  onViewDetails: () => void;
}

export function ContextPopover({
  usage,
  modelLabel,
  accountLabel,
  providerLabel,
  accountUsage,
  providers,
  onViewDetails,
}: Props) {
  const pct = contextPercent(usage);
  const remaining = usage.window === null ? null : usage.window - usage.used;

  return (
    <div className="sr-pop" style={{ left: 330, width: 302 }} role="dialog" aria-label="Context and usage">
      <div className="sr-ctx__top">
        <div style={{ display: "flex", alignItems: "baseline", gap: 6, marginBottom: 9 }}>
          <span className="sr-label">Context window</span>
          <span className="sr-spacer" />
          <span style={{ fontSize: 10.5, color: "var(--sr-text-4)" }}>
            {providerLabel} · {accountLabel} · {modelLabel}
          </span>
        </div>

        <div className="sr-ctx__figure">
          <span className="sr-ctx__used">{formatTokens(usage.used)}</span>
          <span className="sr-ctx__max">
            / {usage.window === null ? "Unavailable" : formatTokens(usage.window)}
          </span>
          <span className="sr-spacer" />
          {pct !== null && (
            <span className="sr-ctx__pct" style={{ color: usageColor(pct) }}>
              {pct}%
            </span>
          )}
        </div>

        {pct !== null && (
          <div className="sr-bar sr-bar--lg" style={{ marginBottom: 13 }}>
            <span style={{ width: `${pct}%`, background: usageColor(pct) }} />
          </div>
        )}

        <Row label="Context used" value={usage.used} />
        <Row label="Context window" value={usage.window} />
        <Row label="Remaining" value={remaining} />
        <div className="sr-ctx__sep" />
        <Row label="Input tokens" value={usage.input} />
        <Row label="Output tokens" value={usage.output} />
        <Row label="Reasoning tokens" value={usage.reasoning} />
        <Row label="Cache read" value={usage.cacheRead} />
      </div>

      <div className="sr-ctx__usage">
        <div style={{ display: "flex", alignItems: "baseline", marginBottom: 11 }}>
          <span className="sr-label">Usage limits</span>
          <span className="sr-spacer" />
          <span style={{ fontSize: 10, color: "var(--sr-text-4)" }}>all linked accounts</span>
        </div>

        {accountUsage.map((entry) => {
          const provider = providers[entry.providerId];
          const account = provider.accounts.find((a) => a.id === entry.accountId);
          if (!account) return null;

          return (
            <div key={`${entry.providerId}-${entry.accountId}`}>
              <p className="sr-ctx__acct" style={{ color: provider.color }}>
                {provider.label} — {account.label}
              </p>

              {entry.windows === null ? (
                <div className="sr-ctx__unavailable">
                  <Info size={12} />
                  <span>Unavailable — this CLI does not report plan usage.</span>
                </div>
              ) : (
                entry.windows.map((w) => (
                  <div className="sr-ctx__win" key={w.label}>
                    <span className="sr-ctx__win-label">{w.label}</span>
                    <span className="sr-bar">
                      {w.percent !== null && (
                        <span style={{ width: `${w.percent}%`, background: usageColor(w.percent) }} />
                      )}
                    </span>
                    <span className="sr-ctx__win-pct">
                      {w.percent === null ? "—" : `${w.percent}%`}
                    </span>
                    <span className="sr-ctx__win-reset">{w.resetsAt ?? "Unavailable"}</span>
                  </div>
                ))
              )}
              <div style={{ height: 7 }} />
            </div>
          );
        })}

        <div style={{ display: "flex", alignItems: "center" }}>
          <button style={{ color: "var(--sr-accent)", fontSize: 11.5 }} onClick={onViewDetails}>
            View detailed usage →
          </button>
          <span className="sr-spacer" />
          <span style={{ fontSize: 10.5, color: "var(--sr-text-4)" }}>
            updated {formatUpdatedAgo(usage.updatedAt)}
          </span>
        </div>
      </div>
    </div>
  );
}
