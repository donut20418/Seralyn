import React from 'react';
import { ConversationSummary } from '../../lib/types';
import { ConversationItem } from './ConversationItem';

interface SidebarProps {
  conversations: ConversationSummary[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
}

export function Sidebar({ conversations, activeId, onSelect, onNew, onDelete }: SidebarProps) {
  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <h2>Seralyn</h2>
        <button className="new-chat-btn" onClick={onNew}>+ New</button>
      </div>
      <div className="conversation-list">
        {conversations.map(conv => (
          <ConversationItem 
            key={conv.id} 
            conversation={conv} 
            isActive={conv.id === activeId}
            onSelect={() => onSelect(conv.id)}
            onDelete={() => onDelete(conv.id)}
          />
        ))}
      </div>
    </div>
  );
}
