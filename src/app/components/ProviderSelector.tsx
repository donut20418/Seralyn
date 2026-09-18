import { useEffect, useRef, useState } from "react";
import { Check, Pencil } from "lucide-react";
import { PROVIDER_ORDER, formatTokens } from "../data/providers";
import type { ConnectionStatus, ProviderId, ProviderInfo, Selection } from "../types";

const STATUS_TEXT: Record<ConnectionStatus, string> = {
  connected: "connected",
  "signin-required": "sign in required",
  "not-installed": "not installed",
  disconnected: "disconnected",
  error: "error",
  checking: "checking…",
};

function statusColor(status: ConnectionStatus) {
  if (status === "connected") return "var(--sr-text-4)";
  if (status === "signin-required" || status === "checking") return "var(--sr-warn)";
  if (status === "error") return "var(--sr-danger)";
  return "var(--sr-text-3)";
}

interface Props {
  providers: Record<ProviderId, ProviderInfo>;
  selection: Selection;
  onSelect: (provider: ProviderId, accountId: string, modelId: string) => void;
  onRenameAccount: (provider: ProviderId, accountId: string, label: string) => void;
  onManageProviders: () => void;
}

export function ProviderSelector({
  providers,
  selection,
  onSelect,
  onRenameAccount,
  onManageProviders,
}: Props) {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) inputRef.current?.select();
  }, [editing]);

  const commit = (provider: ProviderId, accountId: string) => {
    const next = draft.trim();
    if (next) onRenameAccount(provider, accountId, next);
    setEditing(null);
  };

  return (
    <div className="sr-pop" style={{ left: 34, width: 428 }} role="dialog" aria-label="Choose provider, account and model">
      <div className="sr-pop__head">
        <span className="sr-label">Provider · Account · Model</span>
        <span className="sr-spacer" />
        <span className="sr-mono" style={{ fontSize: 10, color: "var(--sr-text-4)" }}>
          ⌘M
        </span>
      </div>

      <div className="sr-pop__body sr-scroll">
        {PROVIDER_ORDER.map((id) => {
          const provider = providers[id];
          const signedOut =
            provider.status === "signin-required" || provider.status === "not-installed";

          return (
            <div key={id}>
              <div className="sr-sel__provider">
                <span
                  className={`sr-dot${provider.status === "connected" ? "" : " sr-dot--hollow"}`}
                  style={{ background: provider.color }}
                />
                <span
                  className="sr-sel__provider-name"
                  style={{ color: provider.status === "connected" ? provider.color : "var(--sr-text-3)" }}
                >
                  {provider.label}
                </span>
                <span className="sr-spacer" />
                <span className="sr-sel__cli" style={{ color: statusColor(provider.status) }}>
                  {provider.cliVersion ? `cli ${provider.cliVersion} · ` : ""}
                  {STATUS_TEXT[provider.status]}
                </span>
              </div>

              {provider.accounts.map((account) => (
                <div key={account.id}>
                  <div className="sr-sel__account">
                    {editing === account.id ? (
                      <input
                        ref={inputRef}
                        className="sr-sel__rename-input"
                        value={draft}
                        onChange={(e) => setDraft(e.target.value)}
                        onBlur={() => commit(id, account.id)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") commit(id, account.id);
                          if (e.key === "Escape") setEditing(null);
                        }}
                        aria-label="Profile label"
                      />
                    ) : (
                      <>
                        <span className="sr-sel__account-label">{account.label}</span>
                        <button
                          className="sr-sel__rename"
                          aria-label={`Rename ${account.label}`}
                          onClick={() => {
                            setDraft(account.label);
                            setEditing(account.id);
                          }}
                        >
                          <Pencil size={10} />
                        </button>
                      </>
                    )}
                  </div>

                  {signedOut ? (
                    <>
                      <div className="sr-sel__signin">
                        <button className="sr-btn" style={{ borderColor: "rgba(224,179,87,0.30)", background: "rgba(224,179,87,0.10)", color: "var(--sr-warn)" }}>
                          Sign in with {provider.label} CLI
                        </button>
                      </div>
                      <p className="sr-sel__note">
                        Models stay hidden until the CLI reports an authenticated account. Seralyn
                        opens the provider's own sign-in and never reads credential files.
                      </p>
                    </>
                  ) : (
                    account.models.map((model) => {
                      const active =
                        selection.provider === id &&
                        selection.accountId === account.id &&
                        selection.modelId === model.id;
                      return (
                        <button
                          key={model.id}
                          className={`sr-sel__model${active ? " sr-sel__model--on" : ""}`}
                          onClick={() => onSelect(id, account.id, model.id)}
                          aria-current={active}
                        >
                          <span>{model.label}</span>
                          <span className="sr-spacer" />
                          <span className="sr-sel__ctx">
                            {model.contextWindow === null ? "—" : formatTokens(model.contextWindow)}
                          </span>
                          {active && <Check size={12} color="var(--sr-accent)" />}
                        </button>
                      );
                    })
                  )}
                </div>
              ))}
            </div>
          );
        })}
      </div>

      <div className="sr-pop__foot">
        <span>Models as reported by each CLI</span>
        <span className="sr-spacer" />
        <button style={{ color: "var(--sr-accent)", fontSize: 11 }} onClick={onManageProviders}>
          Manage providers
        </button>
      </div>
    </div>
  );
}
