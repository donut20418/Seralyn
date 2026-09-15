import React from 'react';
import { ProviderStatus as IProviderStatus } from '../../lib/types';

interface ProviderStatusProps {
  status: IProviderStatus;
}

export function ProviderStatus({ status }: ProviderStatusProps) {
  return (
    <div className={`provider-status-card ${status.provider}`}>
      <h3>{status.provider}</h3>
      <div className="status-details">
        <span>Version: {status.version || 'Unknown'}</span>
        <span className={`auth-badge ${status.authenticated.toLowerCase()}`}>
          {status.authenticated}
        </span>
      </div>
    </div>
  );
}
