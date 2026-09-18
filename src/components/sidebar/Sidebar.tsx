import { useState, useMemo } from 'react';
import { Plus, Search, X } from 'lucide-react';
import type { ConversationSummary } from '../../lib/types';
import { ConversationItem } from './ConversationItem';

interface SidebarProps {
  conversations: ConversationSummary[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
}

export function Sidebar({ conversations, activeId, onSelect, onNew, onDelete }: SidebarProps) {
  const [searchQuery, setSearchQuery] = useState('');

  const filteredConversations = useMemo(() => {
    if (!searchQuery.trim()) return conversations;
    const query = searchQuery.toLowerCase();
    return conversations.filter(
      conv =>
        (conv.title && conv.title.toLowerCase().includes(query)) ||
        (conv.last_message_preview && conv.last_message_preview.toLowerCase().includes(query))
    );
  }, [conversations, searchQuery]);

  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <div className="app-brand">
          <h2>Seralyn</h2>
          <span className="version-tag">v0.2.0</span>
        </div>
        <button className="new-chat-btn" onClick={onNew} title="New conversation">
          <Plus size={15} />
          <span>New</span>
        </button>
      </div>

      {/* Search Filter */}
      <div className="sidebar-search">
        <Search size={14} className="search-icon" />
        <input
          type="text"
          className="search-input"
          placeholder="Search conversations..."
          value={searchQuery}
          onChange={e => setSearchQuery(e.target.value)}
        />
        {searchQuery && (
          <button className="clear-search-btn" onClick={() => setSearchQuery('')} title="Clear search">
            <X size={12} />
          </button>
        )}
      </div>

      {/* Conversation List */}
      <div className="conversation-list">
        {filteredConversations.length === 0 ? (
          <div className="empty-sidebar-notice">
            {searchQuery ? 'No matching conversations' : 'No conversations yet'}
          </div>
        ) : (
          filteredConversations.map(conv => (
            <ConversationItem
              key={conv.id}
              conversation={conv}
              isActive={conv.id === activeId}
              onSelect={() => onSelect(conv.id)}
              onDelete={() => onDelete(conv.id)}
            />
          ))
        )}
      </div>
    </div>
  );
}
