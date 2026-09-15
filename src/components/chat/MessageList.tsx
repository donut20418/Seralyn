import React, { useRef, useEffect } from 'react';
import { Message, ProviderKind } from '../../lib/types';
import { MessageBubble } from './MessageBubble';
import { StreamingText } from './StreamingText';

interface MessageListProps {
  messages: Message[];
  isStreaming: boolean;
  streamingContent: string;
  activeProvider: ProviderKind;
}

export function MessageList({ messages, isStreaming, streamingContent, activeProvider }: MessageListProps) {
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, streamingContent]);

  return (
    <div className="message-list">
      {messages.map((msg) => (
        <MessageBubble key={msg.id} message={msg} />
      ))}
      {isStreaming && (
        <StreamingText content={streamingContent} provider={activeProvider} />
      )}
      <div ref={endRef} />
    </div>
  );
}
