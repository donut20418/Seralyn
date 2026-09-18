import { Paperclip } from 'lucide-react';
import type { Message } from '../../lib/types';
import { MarkdownRenderer } from './MarkdownRenderer';
import { ThinkingAccordion } from './ThinkingAccordion';

interface MessageBubbleProps {
  message: Message;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === 'user';

  // Parse metadata if available for thinking or attachments
  let thinking = message.thinking;
  let attachments = message.attachments;

  if (message.metadata_json) {
    try {
      const meta = JSON.parse(message.metadata_json);
      if (!thinking && meta.thinking) thinking = meta.thinking;
      if (!attachments && meta.attachments) attachments = meta.attachments;
    } catch {
      // Ignore JSON parse errors in metadata
    }
  }

  return (
    <div className={`message-bubble ${isUser ? 'user' : 'assistant'}`}>
      <div className="message-header">
        {!isUser && message.provider ? (
          <div className={`provider-badge ${message.provider}`}>
            [{message.provider.toUpperCase()}]
            {message.model && <span className="model-name">({message.model})</span>}
          </div>
        ) : (
          <span className="user-label">You</span>
        )}
        <span className="message-time">
          {new Date(message.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
        </span>
      </div>

      {/* Attachments preview */}
      {attachments && attachments.length > 0 && (
        <div className="message-attachments">
          {attachments.map(att => (
            <div key={att.id} className="attachment-chip" title={att.path}>
              <Paperclip size={13} className="attachment-chip-icon" />
              <span className="attachment-name">{att.name}</span>
              <span className="attachment-size">({(att.size / 1024).toFixed(1)} KB)</span>
            </div>
          ))}
        </div>
      )}

      {/* Collapsible reasoning block */}
      {thinking && <ThinkingAccordion thinking={thinking} />}

      {/* Message body */}
      <div className="message-content">
        {isUser ? (
          <div className="user-text-content">{message.content}</div>
        ) : (
          <MarkdownRenderer content={message.content} />
        )}
      </div>
    </div>
  );
}
