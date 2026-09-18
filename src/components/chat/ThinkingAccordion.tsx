import { useState } from 'react';
import { ChevronDown, ChevronRight, Brain, Sparkles } from 'lucide-react';

interface ThinkingAccordionProps {
  thinking: string;
  isStreaming?: boolean;
}

export function ThinkingAccordion({ thinking, isStreaming = false }: ThinkingAccordionProps) {
  const [isOpen, setIsOpen] = useState(isStreaming);

  if (!thinking && !isStreaming) return null;

  const wordCount = thinking ? thinking.trim().split(/\s+/).length : 0;

  return (
    <div className={`thinking-accordion ${isStreaming ? 'streaming' : ''}`}>
      <button
        className="thinking-header"
        onClick={() => setIsOpen(!isOpen)}
        aria-expanded={isOpen}
      >
        <div className="thinking-title">
          {isStreaming ? (
            <Sparkles size={16} className="thinking-icon pulse" />
          ) : (
            <Brain size={16} className="thinking-icon" />
          )}
          <span className="thinking-label">
            {isStreaming ? 'Thinking...' : `Thought process (${wordCount} words)`}
          </span>
        </div>
        <span className="thinking-chevron">
          {isOpen ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </span>
      </button>
      {isOpen && (
        <div className="thinking-content">
          <pre>{thinking}</pre>
        </div>
      )}
    </div>
  );
}
