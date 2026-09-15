import type { Message, ProviderKind, ProviderStatus } from '../../lib/types';
import { ProviderSelector } from '../providers/ProviderSelector';
import { MessageList } from './MessageList';
import { ChatInput } from './ChatInput';

interface ChatViewProps {
  conversationId: string | null;
  messages: Message[];
  isStreaming: boolean;
  streamingContent: string;
  providers: ProviderStatus[];
  activeProvider: ProviderKind;
  onSelectProvider: (p: ProviderKind) => void;
  onSendMessage: (content: string, provider: ProviderKind) => void;
  onNewConversation: () => void;
}

export function ChatView({
  conversationId,
  messages,
  isStreaming,
  streamingContent,
  providers,
  activeProvider,
  onSelectProvider,
  onSendMessage,
  onNewConversation,
}: ChatViewProps) {
  const handleSend = (content: string) => {
    if (!conversationId) {
      onNewConversation();
    } else {
      onSendMessage(content, activeProvider);
    }
  };

  return (
    <div className="chat-view">
      <div className="chat-header">
        <ProviderSelector 
          providers={providers} 
          activeProvider={activeProvider} 
          onSelect={onSelectProvider} 
        />
      </div>
      <div className="chat-messages-container">
        <MessageList 
          messages={messages} 
          isStreaming={isStreaming} 
          streamingContent={streamingContent} 
          activeProvider={activeProvider}
        />
      </div>
      <div className="chat-input-container">
        <ChatInput onSend={handleSend} disabled={isStreaming} />
      </div>
    </div>
  );
}
