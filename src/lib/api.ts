import { invoke } from '@tauri-apps/api/core';
import type {
  Conversation,
  ConversationSummary,
  ConversationWithMessages,
  ProviderStatus,
  ProviderKind,
  AttachmentInfo,
  UsageSnapshot,
} from './types';

export async function createConversation(title?: string): Promise<Conversation> {
  return invoke('create_conversation', { title });
}

export async function listConversations(): Promise<ConversationSummary[]> {
  return invoke('list_conversations');
}

export async function getConversation(id: string): Promise<ConversationWithMessages> {
  return invoke('get_conversation', { id });
}

export async function deleteConversation(id: string): Promise<void> {
  return invoke('delete_conversation', { id });
}

export async function updateConversationTitle(id: string, title: string): Promise<void> {
  return invoke('update_conversation_title', { id, title });
}

export async function sendMessage(
  conversationId: string,
  content: string,
  provider: ProviderKind,
  attachments?: AttachmentInfo[]
): Promise<void> {
  return invoke('send_message', { conversationId, content, provider, attachments: attachments || [] });
}

export async function switchProvider(conversationId: string, provider: ProviderKind): Promise<void> {
  return invoke('switch_provider', { conversationId, provider });
}

export async function detectProviders(): Promise<ProviderStatus[]> {
  return invoke('detect_providers');
}

export async function respondToApproval(conversationId: string, provider: ProviderKind, approvalId: string, approved: boolean): Promise<void> {
  return invoke('respond_to_approval', { conversationId, provider, approvalId, approved });
}

export async function interruptTurn(conversationId: string, provider: ProviderKind): Promise<void> {
  return invoke('interrupt_turn', { conversationId, provider });
}

export async function saveAttachment(conversationId: string, fileName: string, fileData: number[], mimeType?: string): Promise<AttachmentInfo> {
  return invoke('save_attachment', { conversationId, fileName, fileData, mimeType });
}

export async function getConversationUsage(conversationId: string): Promise<UsageSnapshot | null> {
  return invoke('get_conversation_usage', { conversationId });
}
