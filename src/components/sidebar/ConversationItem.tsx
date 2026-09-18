import React from 'react';
import { Trash2, MessageSquare } from 'lucide-react';
import type { ConversationSummary } from '../../lib/types';

interface ConversationItemProps {
  conversation: ConversationSummary;
  isActive: boolean;
  onSelect: () => void;
  onDelete: () => void;
}

export function ConversationItem({
  conversation,
  isActive,
  onSelect,
  onDelete,
}: ConversationItemProps) {
  const handleDelete = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (confirm(`Delete "${conversation.title || 'Conversation'}"?`)) {
      onDelete();
    }
  };

  return (
    <div
      className={`conversation-item ${isActive ? 'active' : ''}`}
      onClick={onSelect}
      role="button"
      tabIndex={0}
    >
      <div className="conv-item-top">
        <div className="conv-title" title={conversation.title || 'New Conversation'}>
          {conversation.title || 'New Conversation'}
        </div>
        <button
          className="delete-btn"
          onClick={handleDelete}
          title="Delete conversation"
        >
          <Trash2 size={13} />
        </button>
      </div>

      <div className="conv-preview">
        {conversation.last_message_preview || 'No messages yet...'}
      </div>

      <div className="conv-footer">
        {conversation.provider && (
          <span className={`conv-provider-tag ${conversation.provider}`}>
            {conversation.provider}
          </span>
        )}
        <span className="conv-msg-count">
          <MessageSquare size={11} />
          <span>{conversation.message_count}</span>
        </span>
      </div>
    </div>
  );
}
