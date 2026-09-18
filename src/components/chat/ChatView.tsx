import { useState } from 'react';
import { Layers, Edit2, Check } from 'lucide-react';
import type {
  Conversation,
  Message,
  ProviderKind,
  ProviderStatus,
  ProviderSessionRecord,
  AttachmentInfo,
  UsageSnapshot,
  ToolCallState,
  PendingApproval,
} from '../../lib/types';
import { ProviderSelector } from '../providers/ProviderSelector';
import { MessageList } from './MessageList';
import { ChatInput } from './ChatInput';
import { TokenMeter } from './TokenMeter';
import { ContextInspector } from './ContextInspector';
import { ApprovalDialog } from '../common/ApprovalDialog';
import { ErrorBanner } from '../common/ErrorBanner';

interface ChatViewProps {
  conversation: Conversation | null;
  conversationId: string | null;
  messages: Message[];
  providerSessions: ProviderSessionRecord[];
  isStreaming: boolean;
  streamingProvider?: ProviderKind | null;
  streamingContent: string;
  streamingThinking?: string;
  currentUsage: UsageSnapshot | null;
  activeTools: ToolCallState[];
  pendingApproval: PendingApproval | null;
  errorMessage: string | null;
  onClearError: () => void;
  providers: ProviderStatus[];
  activeProvider: ProviderKind;
  onSelectProvider: (p: ProviderKind) => void;
  onSendMessage: (content: string, provider: ProviderKind, attachments: AttachmentInfo[]) => void;
  onInterrupt: () => void;
  onUploadAttachment: (file: File) => Promise<AttachmentInfo | null>;
  onDeleteAttachment?: (id: string) => Promise<void> | void;
  onRespondToApproval: (approved: boolean) => void;
  onRenameConversation: (title: string) => void;
  onNewConversation: () => void;
}

export function ChatView({
  conversation,
  conversationId,
  messages,
  providerSessions,
  isStreaming,
  streamingProvider,
  streamingContent,
  streamingThinking = '',
  currentUsage,
  activeTools,
  pendingApproval,
  errorMessage,
  onClearError,
  providers,
  activeProvider,
  onSelectProvider,
  onSendMessage,
  onInterrupt,
  onUploadAttachment,
  onDeleteAttachment,
  onRespondToApproval,
  onRenameConversation,
  onNewConversation,
}: ChatViewProps) {
  const [showInspector, setShowInspector] = useState(false);
  const [isEditingTitle, setIsEditingTitle] = useState(false);
  const [editedTitle, setEditedTitle] = useState('');

  const handleStartRename = () => {
    setEditedTitle(conversation?.title || 'New Conversation');
    setIsEditingTitle(true);
  };

  const handleSaveRename = () => {
    if (editedTitle.trim()) {
      onRenameConversation(editedTitle.trim());
    }
    setIsEditingTitle(false);
  };

  const handleSend = (content: string, attachments: AttachmentInfo[]) => {
    if (!conversationId) {
      onNewConversation();
    } else {
      onSendMessage(content, activeProvider, attachments);
    }
  };

  return (
    <div className="chat-view">
      {/* Top Header */}
      <div className="chat-header">
        <div className="chat-header-left">
          {isEditingTitle ? (
            <div className="title-edit-group">
              <input
                type="text"
                className="title-edit-input"
                value={editedTitle}
                onChange={e => setEditedTitle(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter') handleSaveRename();
                  if (e.key === 'Escape') setIsEditingTitle(false);
                }}
                autoFocus
              />
              <button className="save-title-btn" onClick={handleSaveRename} title="Save title">
                <Check size={14} />
              </button>
            </div>
          ) : (
            <div className="title-display-group" onClick={handleStartRename} title="Click to rename">
              <h2 className="current-chat-title">{conversation?.title || 'New Conversation'}</h2>
              <Edit2 size={13} className="title-edit-icon" />
            </div>
          )}
        </div>

        <div className="chat-header-center">
          <TokenMeter usage={currentUsage} />
        </div>

        <div className="chat-header-right">
          <ProviderSelector
            providers={providers}
            activeProvider={activeProvider}
            onSelect={onSelectProvider}
            disabled={isStreaming}
          />
          <button
            className={`inspector-toggle-btn ${showInspector ? 'active' : ''}`}
            onClick={() => setShowInspector(!showInspector)}
            title="Toggle Context Inspector"
          >
            <Layers size={16} />
            <span>Inspector</span>
          </button>
        </div>
      </div>

      {/* Error banner */}
      {errorMessage && (
        <ErrorBanner message={errorMessage} onDismiss={onClearError} />
      )}

      {/* Safe Mode approval dialog modal */}
      {pendingApproval && (
        <ApprovalDialog
          approval={pendingApproval}
          onApprove={() => onRespondToApproval(true)}
          onDeny={() => onRespondToApproval(false)}
        />
      )}

      {/* Messages area */}
      <div className="chat-messages-container">
        <MessageList
          messages={messages}
          isStreaming={isStreaming}
          streamingContent={streamingContent}
          streamingThinking={streamingThinking}
          activeTools={activeTools}
          activeProvider={activeProvider}
        />
      </div>

      {/* Input container */}
      <div className="chat-input-container">
        <ChatInput
          onSend={handleSend}
          onInterrupt={onInterrupt}
          onUploadAttachment={onUploadAttachment}
          onDeleteAttachment={onDeleteAttachment}
          disabled={!conversationId && messages.length === 0}
          isStreaming={isStreaming}
          streamingProvider={streamingProvider}
        />
      </div>

      {/* Context Inspector slide-over drawer */}
      {showInspector && (
        <ContextInspector
          conversation={conversation}
          messages={messages}
          providerSessions={providerSessions}
          onClose={() => setShowInspector(false)}
        />
      )}
    </div>
  );
}
