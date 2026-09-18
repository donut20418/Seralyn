import type { ProviderKind, ToolCallState } from '../../lib/types';
import { MarkdownRenderer } from './MarkdownRenderer';
import { ThinkingAccordion } from './ThinkingAccordion';
import { ToolCard } from './ToolCard';

interface StreamingTextProps {
  content: string;
  thinking?: string;
  provider: ProviderKind;
  activeTools?: ToolCallState[];
}

export function StreamingText({
  content,
  thinking = '',
  provider,
  activeTools = [],
}: StreamingTextProps) {
  return (
    <div className="message-bubble assistant streaming">
      <div className="message-header">
        <div className={`provider-badge ${provider}`}>
          [{provider.toUpperCase()}]
          <span className="streaming-badge">generating...</span>
        </div>
      </div>

      {/* Live reasoning/thinking stream */}
      {thinking && (
        <ThinkingAccordion thinking={thinking} isStreaming={!content} />
      )}

      {/* Active tool calls */}
      {activeTools.length > 0 && (
        <div className="active-tools-container">
          {activeTools.map(tool => (
            <ToolCard key={tool.id} tool={tool} />
          ))}
        </div>
      )}

      {/* Live response markdown stream */}
      <div className="message-content">
        {content ? (
          <>
            <MarkdownRenderer content={content} />
            <span className="cursor" />
          </>
        ) : (
          !thinking && activeTools.length === 0 && (
            <div className="streaming-placeholder">
              <span className="cursor" />
            </div>
          )
        )}
      </div>
    </div>
  );
}
