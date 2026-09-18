import { useRef, useEffect } from 'react';
import type { Message, ProviderKind, ToolCallState } from '../../lib/types';
import { MessageBubble } from './MessageBubble';
import { StreamingText } from './StreamingText';

interface MessageListProps {
  messages: Message[];
  isStreaming: boolean;
  streamingContent: string;
  streamingThinking?: string;
  activeTools?: ToolCallState[];
  activeProvider: ProviderKind;
}

export function MessageList({
  messages,
  isStreaming,
  streamingContent,
  streamingThinking = '',
  activeTools = [],
  activeProvider,
}: MessageListProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const userScrolledUpRef = useRef(false);

  // Monitor user scroll behavior to avoid jumping if user scrolls up to read history
  const handleScroll = () => {
    if (!containerRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = containerRef.current;
    const distanceFromBottom = scrollHeight - (scrollTop + clientHeight);
    userScrolledUpRef.current = distanceFromBottom > 150;
  };

  useEffect(() => {
    if (!userScrolledUpRef.current) {
      endRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages, streamingContent, streamingThinking, activeTools]);

  return (
    <div className="message-list-scroll-wrapper" ref={containerRef} onScroll={handleScroll}>
      <div className="message-list">
        {messages.length === 0 && !isStreaming && (
          <div className="empty-chat-state">
            <div className="empty-chat-icon">💬</div>
            <h3>Unified Multi-Provider AI</h3>
            <p>Select a provider (Claude, Codex, Gemini) and start a conversation.</p>
          </div>
        )}

        {messages.map(msg => (
          <MessageBubble key={msg.id} message={msg} />
        ))}

        {isStreaming && (
          <StreamingText
            content={streamingContent}
            thinking={streamingThinking}
            provider={activeProvider}
            activeTools={activeTools}
          />
        )}
        <div ref={endRef} />
      </div>
    </div>
  );
}
