import { Message } from '../../lib/types';

interface MessageBubbleProps {
  message: Message;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === 'user';
  
  return (
    <div className={`message-bubble ${isUser ? 'user' : 'assistant'}`}>
      {!isUser && message.provider && (
        <div className={`provider-badge ${message.provider}`}>
          [{message.provider}] {message.model && <span className="model-name">({message.model})</span>}
        </div>
      )}
      <div className="message-content">
        <pre>{message.content}</pre>
      </div>
    </div>
  );
}
