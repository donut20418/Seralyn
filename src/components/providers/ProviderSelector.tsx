import React from 'react';
import { ProviderStatus as IProviderStatus, ProviderKind } from '../../lib/types';

interface ProviderSelectorProps {
  providers: IProviderStatus[];
  activeProvider: ProviderKind;
  onSelect: (provider: ProviderKind) => void;
}

export function ProviderSelector({ providers, activeProvider, onSelect }: ProviderSelectorProps) {
  return (
    <div className="provider-selector">
      <select 
        value={activeProvider} 
        onChange={(e) => onSelect(e.target.value as ProviderKind)}
      >
        <option value="claude">Claude</option>
        <option value="codex">Codex</option>
        <option value="gemini">Gemini</option>
      </select>
      <div className="provider-status-indicator">
        {/* Simple dots for available providers */}
        {providers.map(p => (
          <span 
            key={p.provider} 
            title={`${p.provider}: ${p.authenticated}`}
            className={`status-dot ${p.installed ? (p.authenticated === 'Authenticated' ? 'green' : 'yellow') : 'red'}`}
          />
        ))}
      </div>
    </div>
  );
}
