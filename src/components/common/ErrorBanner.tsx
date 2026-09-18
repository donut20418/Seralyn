import { AlertCircle, X } from 'lucide-react';

interface ErrorBannerProps {
  message: string;
  onDismiss: () => void;
}

export function ErrorBanner({ message, onDismiss }: ErrorBannerProps) {
  if (!message) return null;

  return (
    <div className="error-banner">
      <div className="error-content">
        <AlertCircle size={16} className="error-icon" />
        <span className="error-text">{message}</span>
      </div>
      <button className="error-dismiss-btn" onClick={onDismiss} title="Dismiss">
        <X size={14} />
      </button>
    </div>
  );
}
