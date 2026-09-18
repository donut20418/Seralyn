import { useState, useRef, KeyboardEvent, DragEvent, ChangeEvent } from 'react';
import { Send, Square, Paperclip, X, FileText } from 'lucide-react';
import type { AttachmentInfo, ProviderKind } from '../../lib/types';

interface ChatInputProps {
  onSend: (content: string, attachments: AttachmentInfo[]) => void;
  onInterrupt: () => void;
  onUploadAttachment: (file: File) => Promise<AttachmentInfo | null>;
  disabled: boolean;
  isStreaming: boolean;
  streamingProvider?: ProviderKind | null;
}

export function ChatInput({
  onSend,
  onInterrupt,
  onUploadAttachment,
  disabled,
  isStreaming,
  streamingProvider,
}: ChatInputProps) {
  const [content, setContent] = useState('');
  const [attachments, setAttachments] = useState<AttachmentInfo[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [isUploading, setIsUploading] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleSend = () => {
    if ((content.trim() || attachments.length > 0) && !disabled && !isStreaming) {
      onSend(content.trim(), attachments);
      setContent('');
      setAttachments([]);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const processFiles = async (files: FileList | null) => {
    if (!files || files.length === 0) return;
    setIsUploading(true);
    try {
      for (let i = 0; i < files.length; i++) {
        const file = files[i];
        const info = await onUploadAttachment(file);
        if (info) {
          setAttachments(prev => [...prev, info]);
        }
      }
    } finally {
      setIsUploading(false);
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

  const removeAttachment = (id: string) => {
    setAttachments(prev => prev.filter(a => a.id !== id));
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
      {attachments.length > 0 && (
        <div className="input-attachments-tray">
          {attachments.map(att => (
            <div key={att.id} className="attachment-chip active">
              <FileText size={13} className="attachment-chip-icon" />
              <span className="attachment-name">{att.name}</span>
              <span className="attachment-size">({(att.size / 1024).toFixed(1)} KB)</span>
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

      <div className="input-row">
        <button
          type="button"
          className="attach-btn"
          onClick={() => fileInputRef.current?.click()}
          disabled={disabled || isStreaming || isUploading}
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
            disabled={disabled || (!content.trim() && attachments.length === 0) || isUploading}
            title="Send message"
          >
            <Send size={16} />
            <span>Send</span>
          </button>
        )}
      </div>

      {isUploading && (
        <div className="uploading-indicator">Uploading attachments...</div>
      )}
    </div>
  );
}
