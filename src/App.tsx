import { useEffect } from 'react';
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
    currentConversation,
    providerSessions,
    messages,
    isStreaming,
    streamingProvider,
    streamingContent,
    streamingThinking,
    currentUsage,
    activeTools,
    pendingApproval,
    errorMessage,
    setErrorMessage,
    loadConversations,
    createNewConversation,
    renameConversation,
    selectConversation,
    sendMessage,
    uploadAttachment,
    interrupt,
    respondToApproval,
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
      setErrorMessage(String(e));
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
          conversation={currentConversation}
          conversationId={currentConversationId}
          messages={messages}
          providerSessions={providerSessions}
          isStreaming={isStreaming}
          streamingProvider={streamingProvider}
          streamingContent={streamingContent}
          streamingThinking={streamingThinking}
          currentUsage={currentUsage}
          activeTools={activeTools}
          pendingApproval={pendingApproval}
          errorMessage={errorMessage}
          onClearError={() => setErrorMessage(null)}
          providers={providers}
          activeProvider={activeProvider}
          onSelectProvider={setActiveProvider}
          onSendMessage={sendMessage}
          onInterrupt={() => interrupt(streamingProvider || activeProvider)}
          onUploadAttachment={uploadAttachment}
          onRespondToApproval={approved => respondToApproval(activeProvider, approved)}
          onRenameConversation={title => currentConversationId && renameConversation(currentConversationId, title)}
          onNewConversation={() => createNewConversation()}
        />
      </div>
    </div>
  );
}
