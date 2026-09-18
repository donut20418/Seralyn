import { useState } from 'react';
import { Gauge, Info } from 'lucide-react';
import type { UsageSnapshot } from '../../lib/types';

interface TokenMeterProps {
  usage: UsageSnapshot | null;
}

function formatTokens(count?: number): string {
  if (count === undefined || count === null) return '0';
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M`;
  if (count >= 1_000) return `${(count / 1_000).toFixed(1)}k`;
  return count.toString();
}

export function TokenMeter({ usage }: TokenMeterProps) {
  const [showTooltip, setShowTooltip] = useState(false);

  if (!usage || !usage.context_window) {
    return (
      <div className="token-meter idle">
        <Gauge size={15} className="meter-icon" />
        <span className="meter-text">Context Ready</span>
      </div>
    );
  }

  const contextTokens = usage.context_tokens || (usage.input_tokens || 0) + (usage.output_tokens || 0);
  const maxTokens = usage.context_window;
  const ratio = Math.min(1, Math.max(0, contextTokens / maxTokens));
  const percent = Math.round(ratio * 100);

  let colorClass = 'green';
  if (percent >= 90) colorClass = 'red';
  else if (percent >= 80) colorClass = 'orange';
  else if (percent >= 60) colorClass = 'yellow';

  return (
    <div
      className="token-meter active"
      onMouseEnter={() => setShowTooltip(true)}
      onMouseLeave={() => setShowTooltip(false)}
    >
      <div className="meter-summary">
        <Gauge size={15} className="meter-icon" />
        <span className="meter-label">Context:</span>
        <span className="meter-value">
          {formatTokens(contextTokens)} / {formatTokens(maxTokens)} ({percent}%)
        </span>
        <div className="progress-bar-track">
          <div
            className={`progress-bar-fill ${colorClass}`}
            style={{ width: `${percent}%` }}
          />
        </div>
      </div>

      {showTooltip && (
        <div className="meter-tooltip">
          <div className="tooltip-title">
            <Info size={13} /> Token Breakdown ({usage.confidence})
          </div>
          <div className="tooltip-grid">
            <span className="grid-label">Context Used:</span>
            <span className="grid-val">{contextTokens.toLocaleString()} tokens</span>
            <span className="grid-label">Context Window:</span>
            <span className="grid-val">{maxTokens.toLocaleString()} tokens</span>
            <span className="grid-label">Input Tokens:</span>
            <span className="grid-val">{(usage.input_tokens || 0).toLocaleString()}</span>
            <span className="grid-label">Output Tokens:</span>
            <span className="grid-val">{(usage.output_tokens || 0).toLocaleString()}</span>
            {usage.reasoning_tokens !== undefined && (
              <>
                <span className="grid-label">Reasoning:</span>
                <span className="grid-val">{usage.reasoning_tokens.toLocaleString()}</span>
              </>
            )}
            {usage.cache_read_tokens !== undefined && (
              <>
                <span className="grid-label">Cache Read:</span>
                <span className="grid-val">{usage.cache_read_tokens.toLocaleString()}</span>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
