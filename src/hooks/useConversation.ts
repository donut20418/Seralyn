import { useState, useCallback } from 'react';
import type {
  Conversation,
  ConversationSummary,
  Message,
  ProviderKind,
  NormalizedEvent,
  ProviderSessionRecord,
  AttachmentInfo,
  UsageSnapshot,
  ToolCallState,
  PendingApproval,
} from '../lib/types';
import * as api from '../lib/api';

export function useConversation() {
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(null);
  const [currentConversation, setCurrentConversation] = useState<Conversation | null>(null);
  const [providerSessions, setProviderSessions] = useState<ProviderSessionRecord[]>([]);
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [messages, setMessages] = useState<Message[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [streamingContent, setStreamingContent] = useState('');
  const [streamingThinking, setStreamingThinking] = useState('');
  const [currentUsage, setCurrentUsage] = useState<UsageSnapshot | null>(null);
  const [activeTools, setActiveTools] = useState<ToolCallState[]>([]);
  const [pendingApproval, setPendingApproval] = useState<PendingApproval | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [streamingProvider, setStreamingProvider] = useState<ProviderKind | null>(null);
  const [streamingAccount, setStreamingAccount] = useState<string | null>(null);
  const [streamingModel, setStreamingModel] = useState<string | null>(null);

  const loadConversations = useCallback(async () => {
    try {
      const summaries = await api.listConversations();
      setConversations(summaries);
    } catch (e) {
      console.error(e);
      setErrorMessage(String(e));
    }
  }, []);

  const selectConversation = useCallback(async (id: string, provider?: ProviderKind, account?: string, model?: string) => {
    setIsLoading(true);
    setErrorMessage(null);
    try {
      const { conversation, messages, provider_sessions } = await api.getConversation(id);
      setCurrentConversationId(conversation.id);
      setCurrentConversation(conversation);
      setMessages(messages);
      setProviderSessions(provider_sessions || []);

      // Fetch latest usage snapshot scoped to provider, account, and model
      try {
        const usage = await api.getConversationUsage(id, provider, account, model);
        setCurrentUsage(usage);
      } catch (err) {
        console.warn('Could not fetch usage snapshot:', err);
      }
    } catch (e) {
      console.error(e);
      setErrorMessage(String(e));
    } finally {
      setIsLoading(false);
    }
  }, []);

  const fetchUsage = useCallback(async (convId?: string | null, provider?: ProviderKind, account?: string, model?: string) => {
    const targetId = convId || currentConversationId;
    if (!targetId) return;
    try {
      const usage = await api.getConversationUsage(targetId, provider, account, model);
      setCurrentUsage(usage);
    } catch (err) {
      console.warn('Could not fetch usage snapshot:', err);
    }
  }, [currentConversationId]);

  const deleteAttachment = useCallback(async (attachmentId: string) => {
    if (!currentConversationId) return;
    try {
      await api.deleteAttachment(currentConversationId, attachmentId);
    } catch (e) {
      console.error('Failed to delete attachment:', e);
    }
  }, [currentConversationId]);

  const createNewConversation = useCallback(async (title?: string) => {
    try {
      const conv = await api.createConversation(title);
      await loadConversations();
      await selectConversation(conv.id);
      return conv;
    } catch (e) {
      console.error(e);
      setErrorMessage(String(e));
      return null;
    }
  }, [loadConversations, selectConversation]);

  const renameConversation = useCallback(async (id: string, newTitle: string) => {
    try {
      await api.updateConversationTitle(id, newTitle);
      await loadConversations();
      if (currentConversationId === id) {
        setCurrentConversation(prev => prev ? { ...prev, title: newTitle } : null);
      }
    } catch (e) {
      console.error(e);
      setErrorMessage(String(e));
    }
  }, [currentConversationId, loadConversations]);

  const archiveConversation = useCallback(async (id: string) => {
    try {
      await api.archiveConversation(id);
      await loadConversations();
      if (currentConversationId === id) {
        await createNewConversation();
      }
    } catch (e) {
      console.error(e);
      setErrorMessage(String(e));
    }
  }, [currentConversationId, loadConversations, createNewConversation]);

  const uploadAttachment = useCallback(async (file: File, conversationIdOverride?: string): Promise<AttachmentInfo | null> => {
    const targetId = conversationIdOverride || currentConversationId;
    if (!targetId) return null;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const info = await api.saveAttachment(targetId, file.name, bytes, file.type);
      return info;
    } catch (e) {
      console.error('Failed to save attachment:', e);
      setErrorMessage(`Failed to upload attachment: ${e}`);
      return null;
    }
  }, [currentConversationId]);

  const sendMessage = useCallback(async (
    content: string,
    provider: ProviderKind,
    attachments: AttachmentInfo[] = [],
    model?: string,
    account?: string,
    effort?: string,
    conversationIdOverride?: string
  ) => {
    const targetId = conversationIdOverride || currentConversationId;
    if (!targetId) return;
    setIsStreaming(true);
    setStreamingProvider(provider);
    setStreamingAccount(account || null);
    setStreamingModel(model || null);
    setStreamingContent('');
    setStreamingThinking('');
    setActiveTools([]);
    setPendingApproval(null);
    setErrorMessage(null);

    // Optimistic UI for user message with strict seq assignment
    const lastSeq = messages.length > 0 ? messages[messages.length - 1].seq : 0;
    const tempUserMsg: Message = {
      id: Date.now().toString(),
      conversation_id: targetId,
      seq: lastSeq + 1,
      role: 'user',
      content,
      created_at: new Date().toISOString(),
      attachments,
      model,
    };
    setMessages(prev => [...prev, tempUserMsg]);

    try {
      await api.sendMessage(targetId, content, provider, attachments, model, account, effort);
    } catch (e) {
      console.error(e);
      setIsStreaming(false);
      setStreamingProvider(null);
      setStreamingAccount(null);
      setStreamingModel(null);
      setErrorMessage(String(e));
    }
  }, [currentConversationId, messages]);

  const interrupt = useCallback(async (providerOverride?: ProviderKind, accountOverride?: string, modelOverride?: string) => {
    if (!currentConversationId) return;
    const targetProvider = providerOverride || streamingProvider;
    if (!targetProvider) return;
    try {
      await api.interruptTurn(
        currentConversationId,
        targetProvider,
        accountOverride || streamingAccount || undefined,
        modelOverride || streamingModel || undefined,
      );
      setIsStreaming(false);
      setStreamingProvider(null);
      setStreamingAccount(null);
      setStreamingModel(null);
    } catch (e) {
      console.error('Failed to interrupt:', e);
    }
  }, [currentConversationId, streamingProvider, streamingAccount, streamingModel]);

  const respondToApproval = useCallback(async (
    provider: ProviderKind,
    approved: boolean,
    accountOverride?: string,
    modelOverride?: string,
  ) => {
    if (!currentConversationId || !pendingApproval) return;
    const approvalId = pendingApproval.approval_id;
    setPendingApproval(null);
    try {
      await api.respondToApproval(
        currentConversationId,
        provider,
        approvalId,
        approved,
        accountOverride || streamingAccount || undefined,
        modelOverride || streamingModel || undefined,
      );
    } catch (e) {
      console.error('Failed to respond to approval:', e);
      setErrorMessage(String(e));
    }
  }, [currentConversationId, pendingApproval, streamingAccount, streamingModel]);

  const handleEvent = useCallback((event: NormalizedEvent) => {
    if (event.conversation_id !== currentConversationId) return;

    switch (event.event_type) {
      case 'SessionStarted':
        setIsStreaming(true);
        setStreamingContent('');
        setStreamingThinking('');
        setActiveTools([]);
        break;

      case 'TextDelta':
        if (event.payload.kind === 'Text') {
          const delta = event.payload.content;
          setStreamingContent(prev => prev + delta);
        }
        break;

      case 'ThinkingDelta':
        if (event.payload.kind === 'Thinking') {
          const delta = event.payload.content;
          setStreamingThinking(prev => prev + delta);
        }
        break;

      case 'ToolStarted':
        if (event.payload.kind === 'Tool') {
          const tool = event.payload;
          setActiveTools(prev => [
            ...prev.filter(t => t.id !== tool.tool_id),
            {
              id: tool.tool_id,
              tool_name: tool.tool_name,
              input: tool.input,
              status: 'running',
            },
          ]);
        }
        break;

      case 'ToolProgress':
        if (event.payload.kind === 'Tool') {
          const tool = event.payload;
          setActiveTools(prev =>
            prev.map(t =>
              t.id === tool.tool_id
                ? {
                    ...t,
                    output: tool.output !== undefined ? tool.output : t.output,
                    progress: typeof tool.output === 'string' ? tool.output : (tool.status || 'Executing...'),
                    status: 'running',
                  }
                : t
            )
          );
        }
        break;

      case 'ToolResult':
        if (event.payload.kind === 'Tool') {
          const tool = event.payload;
          setActiveTools(prev =>
            prev.map(t =>
              t.id === tool.tool_id
                ? { ...t, output: tool.output, status: tool.status === 'error' ? 'error' : 'completed' }
                : t
            )
          );
        }
        break;

      case 'ApprovalRequired':
        if (event.payload.kind === 'Approval') {
          const app = event.payload;
          setPendingApproval({
            approval_id: app.approval_id,
            tool_name: app.tool_name,
            description: app.description,
            input: app.input,
          });
        }
        break;

      case 'UsageUpdated':
        if (event.payload.kind === 'Usage') {
          const u = event.payload;
          setCurrentUsage({
            conversation_id: event.conversation_id,
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_read_tokens: u.cache_read_tokens,
            reasoning_tokens: u.reasoning_tokens,
            context_tokens: u.context_tokens,
            context_window: u.context_window,
            confidence: u.confidence,
            created_at: event.timestamp,
          });
        }
        break;

      case 'Error':
        setIsStreaming(false);
        const errAccount = streamingAccount || undefined;
        const errModel = streamingModel || undefined;
        setStreamingProvider(null);
        setStreamingAccount(null);
        setStreamingModel(null);
        if (event.payload.kind === 'Error') {
          setErrorMessage(event.payload.message);
        }
        selectConversation(currentConversationId, event.provider, errAccount, errModel);
        loadConversations();
        break;

      case 'SessionFinished':
        setIsStreaming(false);
        const finAccount = streamingAccount || undefined;
        const finModel = streamingModel || undefined;
        setStreamingProvider(null);
        setStreamingAccount(null);
        setStreamingModel(null);
        selectConversation(currentConversationId, event.provider, finAccount, finModel);
        loadConversations();
        setStreamingContent('');
        setStreamingThinking('');
        setActiveTools([]);
        break;
    }
  }, [currentConversationId, selectConversation, loadConversations, streamingAccount, streamingModel]);

  return {
    currentConversationId,
    currentConversation,
    providerSessions,
    conversations,
    messages,
    isLoading,
    isStreaming,
    streamingProvider,
    streamingAccount,
    streamingModel,
    streamingContent,
    streamingThinking,
    currentUsage,
    activeTools,
    pendingApproval,
    errorMessage,
    setErrorMessage,
    loadConversations,
    selectConversation,
    createNewConversation,
    renameConversation,
    archiveConversation,
    sendMessage,
    uploadAttachment,
    deleteAttachment,
    fetchUsage,
    interrupt,
    respondToApproval,
    handleEvent,
  };
}
