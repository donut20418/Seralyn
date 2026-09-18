import { useState, useMemo, useEffect } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import Prism from 'prismjs';
import 'prismjs/themes/prism-tomorrow.css';
// Common language components for Prism
import 'prismjs/components/prism-javascript';
import 'prismjs/components/prism-typescript';
import 'prismjs/components/prism-jsx';
import 'prismjs/components/prism-tsx';
import 'prismjs/components/prism-rust';
import 'prismjs/components/prism-python';
import 'prismjs/components/prism-bash';
import 'prismjs/components/prism-json';
import 'prismjs/components/prism-markdown';
import 'prismjs/components/prism-css';
import 'prismjs/components/prism-sql';
import { Copy, Check } from 'lucide-react';

interface MarkdownRendererProps {
  content: string;
}

interface CodeBlockProps {
  language: string;
  code: string;
}

function CodeBlock({ language, code }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    Prism.highlightAll();
  }, [code]);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      console.error('Failed to copy code:', e);
    }
  };

  const cleanLang = (language || 'text').replace('language-', '').toLowerCase();

  return (
    <div className="code-block-container">
      <div className="code-block-header">
        <span className="code-block-lang">{cleanLang}</span>
        <button className="copy-code-btn" onClick={handleCopy} title="Copy code">
          {copied ? (
            <>
              <Check size={14} className="copy-icon" />
              <span>Copied!</span>
            </>
          ) : (
            <>
              <Copy size={14} className="copy-icon" />
              <span>Copy</span>
            </>
          )}
        </button>
      </div>
      <pre className={`language-${cleanLang}`}>
        <code className={`language-${cleanLang}`}>{code}</code>
      </pre>
    </div>
  );
}

export function MarkdownRenderer({ content }: MarkdownRendererProps) {
  const components = useMemo(() => ({
    code({ className, children, ...props }: any) {
      const match = /language-(\w+)/.exec(className || '');
      const isInline = !match && typeof children === 'string' && !children.includes('\n');

      if (isInline) {
        return (
          <code className="inline-code" {...props}>
            {children}
          </code>
        );
      }

      const rawCode = String(children).replace(/\n$/, '');
      const lang = match ? match[1] : '';

      return <CodeBlock language={lang} code={rawCode} />;
    },
    a({ href, children, ...props }: any) {
      return (
        <a
          href={href}
          target="_blank"
          rel="noopener noreferrer"
          className="chat-link"
          {...props}
        >
          {children}
        </a>
      );
    },
    table({ children }: any) {
      return (
        <div className="table-responsive">
          <table className="chat-table">{children}</table>
        </div>
      );
    },
  }), []);

  return (
    <div className="markdown-body">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {content}
      </ReactMarkdown>
    </div>
  );
}
