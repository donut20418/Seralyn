import { ConversationSummary } from '../../lib/types';

interface ConversationItemProps {
  conversation: ConversationSummary;
  isActive: boolean;
  onSelect: () => void;
  onDelete: () => void;
}

export function ConversationItem({ conversation, isActive, onSelect, onDelete }: ConversationItemProps) {
  const handleDelete = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (confirm('Delete this conversation?')) {
      onDelete();
    }
  };

  return (
    <div className={`conversation-item ${isActive ? 'active' : ''}`} onClick={onSelect}>
      <div className="conv-title">{conversation.title || 'New Conversation'}</div>
      <div className="conv-preview">{conversation.last_message_preview || '...'}</div>
      <button className="delete-btn" onClick={handleDelete}>×</button>
    </div>
  );
}
