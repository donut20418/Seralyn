import { useState, useCallback } from 'react';
import type { ConversationSummary, Message, ProviderKind, NormalizedEvent } from '../lib/types';
import * as api from '../lib/api';

export function useConversation() {
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(null);
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [messages, setMessages] = useState<Message[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [streamingContent, setStreamingContent] = useState('');

  const loadConversations = useCallback(async () => {
    try {
      const summaries = await api.listConversations();
      setConversations(summaries);
    } catch (e) {
      console.error(e);
    }
  }, []);

  const selectConversation = useCallback(async (id: string) => {
    setIsLoading(true);
    try {
      const { conversation, messages } = await api.getConversation(id);
      setCurrentConversationId(conversation.id);
      setMessages(messages);
    } catch (e) {
      console.error(e);
    } finally {
      setIsLoading(false);
    }
  }, []);

  const createNewConversation = useCallback(async (title?: string) => {
    try {
      const conv = await api.createConversation(title);
      await loadConversations();
      await selectConversation(conv.id);
    } catch (e) {
      console.error(e);
    }
  }, [loadConversations, selectConversation]);

  const sendMessage = useCallback(async (content: string, provider: ProviderKind) => {
    if (!currentConversationId) return;
    setIsStreaming(true);
    setStreamingContent('');
    
    // Optimistic UI for user message
    const tempUserMsg: Message = {
      id: Date.now().toString(),
      conversation_id: currentConversationId,
      role: 'user',
      content,
      created_at: new Date().toISOString(),
    };
    setMessages(prev => [...prev, tempUserMsg]);

    try {
      await api.sendMessage(currentConversationId, content, provider);
    } catch (e) {
      console.error(e);
      setIsStreaming(false);
    }
  }, [currentConversationId]);

  const handleEvent = useCallback((event: NormalizedEvent) => {
    if (event.conversation_id !== currentConversationId) return;

    switch (event.event_type) {
      case 'SessionStarted':
        setIsStreaming(true);
        setStreamingContent('');
        break;
      case 'TextDelta':
        if (event.payload.kind === 'Text') {
          setStreamingContent(prev => prev + event.payload.content);
        }
        break;
      case 'SessionFinished':
        setIsStreaming(false);
        selectConversation(currentConversationId); 
        setStreamingContent('');
        break;
    }
  }, [currentConversationId, selectConversation]);

  return {
    currentConversationId,
    conversations,
    messages,
    isLoading,
    isStreaming,
    streamingContent,
    loadConversations,
    selectConversation,
    createNewConversation,
    sendMessage,
    handleEvent
  };
}
