import { ChevronDown, FolderOpen, Settings } from "lucide-react";
import { PROVIDER_ORDER } from "../data/providers";
import type { ProviderId, ProviderInfo } from "../types";

interface Props {
  workspace: string;
  providers: Record<ProviderId, ProviderInfo>;
  onOpenProvider: (id: ProviderId) => void;
  onOpenSettings: () => void;
}

export function TitleBar({ workspace, providers, onOpenProvider, onOpenSettings }: Props) {
  return (
    <div className="sr-titlebar">
      <div style={{ display: "flex", gap: 6, paddingRight: 4 }} aria-hidden="true">
        <span style={{ width: 10, height: 10, borderRadius: 999, background: "#2c3238" }} />
        <span style={{ width: 10, height: 10, borderRadius: 999, background: "#2c3238" }} />
        <span style={{ width: 10, height: 10, borderRadius: 999, background: "#2c3238" }} />
      </div>
      <span className="sr-divider" />

      <div className="sr-brand">
        <svg width={14} height={14} viewBox="0 0 16 16" fill="none" aria-hidden="true">
          <path d="M8 1.6 L14 5 L8 8.4 L2 5 Z" stroke="var(--sr-accent)" strokeWidth="1.3" strokeLinejoin="round" />
          <path d="M2 11 L8 14.4 L14 11" stroke="var(--sr-accent)" strokeWidth="1.3" strokeLinejoin="round" opacity="0.5" />
        </svg>
        <span className="sr-brand__name">Seralyn</span>
      </div>

      <button className="sr-chip sr-chip--sm">
        <FolderOpen size={12} />
        <span className="sr-mono">{workspace}</span>
        <ChevronDown size={10} className="sr-chip__caret" />
      </button>

      <span className="sr-spacer" />

      <div className="sr-status-bar">
        {PROVIDER_ORDER.map((id) => {
          const p = providers[id];
          const connected = p.status === "connected";
          return (
            <button
              key={id}
              className="sr-status"
              onClick={() => onOpenProvider(id)}
              title={`${p.label} — ${connected ? "connected" : p.status.replace("-", " ")}`}
            >
              <span
                className={`sr-dot${connected ? "" : " sr-dot--hollow"}`}
                style={{ background: p.color }}
              />
              <span>{p.label}</span>
            </button>
          );
        })}
      </div>

      <span className="sr-divider" />
      <button className="sr-icon-btn" style={{ width: 24, height: 24 }} aria-label="Settings" onClick={onOpenSettings}>
        <Settings size={14} />
      </button>
    </div>
  );
}
