import { useState, useRef, useEffect, KeyboardEvent, DragEvent, ChangeEvent } from 'react';
import { Send, Square, Paperclip, X, FileText, Loader2, AlertCircle, Image as ImageIcon } from 'lucide-react';
import type { AttachmentInfo, ProviderKind } from '../../lib/types';

export type AttachmentUploadStatus = 'uploading' | 'ready' | 'unsupported' | 'failed';

export interface StagedAttachmentItem {
  id: string;
  name: string;
  size: number;
  mimeType: string;
  isImage: boolean;
  status: AttachmentUploadStatus;
  errorMessage?: string;
  info?: AttachmentInfo;
}

interface ChatInputProps {
  onSend: (content: string, attachments: AttachmentInfo[]) => void;
  onInterrupt: () => void;
  onUploadAttachment: (file: File) => Promise<AttachmentInfo | null>;
  onDeleteAttachment?: (id: string) => Promise<void> | void;
  disabled: boolean;
  isStreaming: boolean;
  streamingProvider?: ProviderKind | null;
  activeProvider?: ProviderKind;
}

const MAX_SINGLE_FILE_SIZE = 25 * 1024 * 1024; // 25 MB
const MAX_AGGREGATE_SIZE = 50 * 1024 * 1024;   // 50 MB
const MAX_ATTACHMENTS = 10;

export function ChatInput({
  onSend,
  onInterrupt,
  onUploadAttachment,
  onDeleteAttachment,
  disabled,
  isStreaming,
  streamingProvider,
  activeProvider = 'codex',
}: ChatInputProps) {
  const [content, setContent] = useState('');
  const [items, setItems] = useState<StagedAttachmentItem[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Re-evaluate provider support whenever activeProvider changes
  useEffect(() => {
    setItems(prevItems =>
      prevItems.map(item => {
        if (item.status === 'uploading' || item.status === 'failed') {
          return item;
        }
        if (activeProvider === 'claude' && item.isImage) {
          return {
            ...item,
            status: 'unsupported',
            errorMessage: 'Claude does not support image attachments',
          };
        }
        // If switched away from Claude or item is non-image, restore to ready
        if (item.status === 'unsupported' && (activeProvider !== 'claude' || !item.isImage)) {
          return {
            ...item,
            status: 'ready',
            errorMessage: undefined,
          };
        }
        return item;
      })
    );
  }, [activeProvider]);

  const hasUploading = items.some(i => i.status === 'uploading');
  const hasUnsupported = items.some(i => i.status === 'unsupported');
  const hasFailed = items.some(i => i.status === 'failed');
  const readyAttachments = items
    .filter(i => i.status === 'ready' && i.info)
    .map(i => i.info!);

  const canSend =
    (content.trim().length > 0 || readyAttachments.length > 0) &&
    !disabled &&
    !isStreaming &&
    !hasUploading &&
    !hasUnsupported &&
    !hasFailed;

  const handleSend = () => {
    if (!canSend) return;
    onSend(content.trim(), readyAttachments);
    setContent('');
    setItems([]);
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const processFiles = async (files: FileList | null) => {
    if (!files || files.length === 0) return;

    let currentTotalSize = items.reduce((acc, cur) => acc + cur.size, 0);
    let currentCount = items.length;

    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      const tempId = `temp-${Date.now()}-${Math.random().toString(36).substring(2, 8)}`;
      const isImage = file.type.startsWith('image/') || /\.(png|jpe?g|gif|webp|svg|bmp)$/i.test(file.name);

      // Quota validations
      if (currentCount >= MAX_ATTACHMENTS) {
        setItems(prev => [
          ...prev,
          {
            id: tempId,
            name: file.name,
            size: file.size,
            mimeType: file.type || 'application/octet-stream',
            isImage,
            status: 'failed',
            errorMessage: `Exceeds max ${MAX_ATTACHMENTS} files`,
          },
        ]);
        continue;
      }

      if (file.size > MAX_SINGLE_FILE_SIZE) {
        setItems(prev => [
          ...prev,
          {
            id: tempId,
            name: file.name,
            size: file.size,
            mimeType: file.type || 'application/octet-stream',
            isImage,
            status: 'failed',
            errorMessage: 'Exceeds 25MB limit',
          },
        ]);
        continue;
      }

      if (currentTotalSize + file.size > MAX_AGGREGATE_SIZE) {
        setItems(prev => [
          ...prev,
          {
            id: tempId,
            name: file.name,
            size: file.size,
            mimeType: file.type || 'application/octet-stream',
            isImage,
            status: 'failed',
            errorMessage: 'Exceeds 50MB total limit',
          },
        ]);
        continue;
      }

      currentTotalSize += file.size;
      currentCount += 1;

      // Add as uploading
      setItems(prev => [
        ...prev,
        {
          id: tempId,
          name: file.name,
          size: file.size,
          mimeType: file.type || 'application/octet-stream',
          isImage,
          status: 'uploading',
        },
      ]);

      try {
        const info = await onUploadAttachment(file);
        if (info) {
          const unsupported = activeProvider === 'claude' && (isImage || info.kind === 'image');
          setItems(prev =>
            prev.map(item =>
              item.id === tempId
                ? {
                    ...item,
                    id: info.id,
                    info,
                    isImage: isImage || info.kind === 'image',
                    status: unsupported ? 'unsupported' : 'ready',
                    errorMessage: unsupported ? 'Claude does not support image attachments' : undefined,
                  }
                : item
            )
          );
        } else {
          setItems(prev =>
            prev.map(item =>
              item.id === tempId
                ? { ...item, status: 'failed', errorMessage: 'Upload returned null' }
                : item
            )
          );
        }
      } catch (err) {
        setItems(prev =>
          prev.map(item =>
            item.id === tempId
              ? { ...item, status: 'failed', errorMessage: (err as Error)?.message || 'Upload failed' }
              : item
          )
        );
      }
    }
  };

  const handleFileChange = (e: ChangeEvent<HTMLInputElement>) => {
    processFiles(e.target.files);
    if (fileInputRef.current) fileInputRef.current.value = '';
  };

  const handleDragOver = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const handleDragLeave = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDragging(false);
  };

  const handleDrop = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDragging(false);
    processFiles(e.dataTransfer.files);
  };

  const removeAttachment = async (id: string) => {
    const target = items.find(i => i.id === id);
    setItems(prev => prev.filter(i => i.id !== id));
    if (target?.info && onDeleteAttachment) {
      try {
        await onDeleteAttachment(target.info.id);
      } catch (err) {
        console.warn('Failed to delete attachment from disk:', err);
      }
    }
  };

  return (
    <div
      className={`chat-input-wrapper ${isDragging ? 'dragging-over' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Hidden file picker */}
      <input
        type="file"
        ref={fileInputRef}
        onChange={handleFileChange}
        multiple
        style={{ display: 'none' }}
      />

      {/* Attachment chips preview */}
      {items.length > 0 && (
        <div className="input-attachments-tray">
          {items.map(att => (
            <div
              key={att.id}
              className={`attachment-chip ${
                att.status === 'ready'
                  ? 'active'
                  : att.status === 'uploading'
                  ? 'uploading'
                  : 'error'
              }`}
              title={att.errorMessage || att.name}
            >
              {att.status === 'uploading' ? (
                <Loader2 size={13} className="attachment-chip-icon animate-spin" />
              ) : att.status === 'unsupported' || att.status === 'failed' ? (
                <AlertCircle size={13} className="attachment-chip-icon text-amber-500" />
              ) : att.isImage ? (
                <ImageIcon size={13} className="attachment-chip-icon" />
              ) : (
                <FileText size={13} className="attachment-chip-icon" />
              )}

              <span className="attachment-name">{att.name}</span>
              <span className="attachment-size">({(att.size / 1024).toFixed(1)} KB)</span>

              {att.errorMessage && (
                <span className="attachment-error-badge" style={{ color: '#f59e0b', fontSize: '11px', marginLeft: '4px' }}>
                  ({att.errorMessage})
                </span>
              )}

              <button
                className="remove-att-btn"
                onClick={() => removeAttachment(att.id)}
                title="Remove attachment"
              >
                <X size={12} />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Validation warning banner */}
      {hasUnsupported && (
        <div className="attachment-warning-banner" style={{ fontSize: '12px', color: '#f59e0b', padding: '4px 12px', background: 'rgba(245, 158, 11, 0.1)', borderRadius: '4px', marginBottom: '6px' }}>
          ⚠️ Claude does not support image attachments. Remove image attachments or switch to Codex/Gemini to send.
        </div>
      )}

      {hasFailed && !hasUnsupported && (
        <div className="attachment-warning-banner" style={{ fontSize: '12px', color: '#ef4444', padding: '4px 12px', background: 'rgba(239, 68, 68, 0.1)', borderRadius: '4px', marginBottom: '6px' }}>
          ⚠️ Some attachments failed validation. Remove invalid attachments to send.
        </div>
      )}

      <div className="input-row">
        <button
          type="button"
          className="attach-btn"
          onClick={() => fileInputRef.current?.click()}
          disabled={disabled || isStreaming || hasUploading}
          title="Attach file (or drag & drop)"
        >
          <Paperclip size={18} />
        </button>

        <textarea
          className="chat-input"
          value={content}
          onChange={e => setContent(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={
            isStreaming
              ? 'Model is generating...'
              : isDragging
              ? 'Drop files here to attach...'
              : 'Type a message... (Enter to send, Shift+Enter for new line)'
          }
          disabled={disabled || isStreaming}
          rows={1}
        />

        {isStreaming ? (
          <button
            type="button"
            className="stop-button"
            onClick={onInterrupt}
            title={streamingProvider ? `Stop ${streamingProvider} generation` : 'Stop generation'}
          >
            <Square size={16} fill="currentColor" />
            <span>Stop</span>
          </button>
        ) : (
          <button
            type="button"
            className="send-button"
            onClick={handleSend}
            disabled={!canSend}
            title={
              hasUnsupported
                ? 'Cannot send: remove unsupported attachments'
                : hasFailed
                ? 'Cannot send: remove failed attachments'
                : hasUploading
                ? 'Uploading attachments...'
                : 'Send message'
            }
          >
            <Send size={16} />
            <span>Send</span>
          </button>
        )}
      </div>

      {hasUploading && (
        <div className="uploading-indicator">Uploading attachments...</div>
      )}
    </div>
  );
}
