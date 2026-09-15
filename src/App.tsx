import React, { useEffect } from 'react';
import { Sidebar } from './components/sidebar/Sidebar';
import { ChatView } from './components/chat/ChatView';
import { useConversation } from './hooks/useConversation';
import { useProviders } from './hooks/useProviders';
import { useEvents } from './hooks/useEvents';
import { deleteConversation } from './lib/api';

export default function App() {
  const { 
    conversations, 
    currentConversationId, 
    messages,
    isStreaming,
    streamingContent,
    loadConversations, 
    createNewConversation,
    selectConversation,
    sendMessage,
    handleEvent,
  } = useConversation();

  const { providers, activeProvider, setActiveProvider, detectProviders } = useProviders();

  useEvents(handleEvent);

  useEffect(() => {
    detectProviders();
    loadConversations();
  }, [detectProviders, loadConversations]);

  const handleDelete = async (id: string) => {
    try {
      await deleteConversation(id);
      await loadConversations();
      if (currentConversationId === id) {
        createNewConversation();
      }
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className="app-container">
      <Sidebar 
        conversations={conversations} 
        activeId={currentConversationId}
        onSelect={selectConversation}
        onNew={() => createNewConversation()}
        onDelete={handleDelete}
      />
      <div className="main-content">
        <ChatView 
          conversationId={currentConversationId}
          messages={messages}
          isStreaming={isStreaming}
          streamingContent={streamingContent}
          providers={providers}
          activeProvider={activeProvider}
          onSelectProvider={setActiveProvider}
          onSendMessage={sendMessage}
          onNewConversation={() => createNewConversation()} 
        />
      </div>
    </div>
  );
}
