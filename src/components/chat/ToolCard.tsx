import { useState } from 'react';
import { Wrench, CheckCircle2, XCircle, Loader2, ChevronDown, ChevronRight } from 'lucide-react';
import type { ToolCallState } from '../../lib/types';

interface ToolCardProps {
  tool: ToolCallState;
}

export function ToolCard({ tool }: ToolCardProps) {
  const [expanded, setExpanded] = useState(false);

  const formatPayload = (data: any) => {
    if (!data) return null;
    if (typeof data === 'string') {
      try {
        const parsed = JSON.parse(data);
        return JSON.stringify(parsed, null, 2);
      } catch {
        return data;
      }
    }
    return JSON.stringify(data, null, 2);
  };

  const formattedInput = formatPayload(tool.input);
  const formattedOutput = formatPayload(tool.output);

  return (
    <div className={`tool-card tool-${tool.status}`}>
      <div className="tool-card-header" onClick={() => setExpanded(!expanded)}>
        <div className="tool-info">
          <Wrench size={15} className="tool-icon" />
          <span className="tool-name">{tool.tool_name}</span>
        </div>
        <div className="tool-status">
          {tool.status === 'running' && (
            <span className="status-badge running" title={tool.progress}>
              <Loader2 size={13} className="spin" />
              <span>{tool.progress ? `Running: ${tool.progress}` : 'Running'}</span>
            </span>
          )}
          {tool.status === 'completed' && (
            <span className="status-badge completed">
              <CheckCircle2 size={13} />
              <span>Done</span>
            </span>
          )}
          {tool.status === 'error' && (
            <span className="status-badge error">
              <XCircle size={13} />
              <span>Error</span>
            </span>
          )}
          <span className="tool-chevron">
            {expanded ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
          </span>
        </div>
      </div>

      {expanded && (
        <div className="tool-details">
          {tool.progress && tool.status === 'running' && (
            <div className="tool-section">
              <div className="tool-section-title">Live Progress:</div>
              <pre className="tool-payload">{tool.progress}</pre>
            </div>
          )}
          {formattedInput && (
            <div className="tool-section">
              <div className="tool-section-title">Arguments:</div>
              <pre className="tool-payload">{formattedInput}</pre>
            </div>
          )}
          {formattedOutput && (
            <div className="tool-section">
              <div className="tool-section-title">Output:</div>
              <pre className="tool-payload">{formattedOutput}</pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
