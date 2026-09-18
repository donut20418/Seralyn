import { Cpu } from 'lucide-react';
import type { ProviderStatus as IProviderStatus, ProviderKind } from '../../lib/types';

interface ProviderSelectorProps {
  providers: IProviderStatus[];
  activeProvider: ProviderKind;
  onSelect: (provider: ProviderKind) => void;
  disabled?: boolean;
}

export function ProviderSelector({ providers, activeProvider, onSelect, disabled = false }: ProviderSelectorProps) {
  return (
    <div className="provider-selector-container">
      <div className="selector-field">
        <Cpu size={15} className={`provider-icon ${activeProvider}`} />
        <select
          className={`provider-select ${activeProvider}`}
          value={activeProvider}
          onChange={e => onSelect(e.target.value as ProviderKind)}
          disabled={disabled}
        >
          <option value="claude">Claude Code CLI</option>
          <option value="codex">OpenAI Codex CLI</option>
          <option value="gemini">Google Gemini ACP</option>
        </select>
      </div>

      <div className="provider-status-dots">
        {providers.map(p => {
          const isInstalled = p.installed;
          const isAuth = p.authenticated === 'Authenticated';
          let statusClass = 'not-installed';
          let title = `${p.provider}: Not installed`;

          if (isInstalled && isAuth) {
            statusClass = 'online';
            title = `${p.provider}: Ready & Authenticated`;
          } else if (isInstalled) {
            statusClass = 'needs-auth';
            title = `${p.provider}: Installed (Auth: ${p.authenticated})`;
          }

          return (
            <span
              key={p.provider}
              title={title}
              className={`status-pill ${p.provider} ${statusClass} ${p.provider === activeProvider ? 'current' : ''}`}
            >
              <span className="dot" />
              <span className="name">{p.provider[0].toUpperCase()}</span>
            </span>
          );
        })}
      </div>
    </div>
  );
}
