import { useCallback, useEffect, useRef, useState } from "react";
import { BrowserPanel } from "./app/components/BrowserPanel";
import { ChatHeader } from "./app/components/ChatHeader";
import { Composer } from "./app/components/Composer";
import { Conversation } from "./app/components/Conversation";
import { Inspector } from "./app/components/Inspector";
import { Sidebar } from "./app/components/Sidebar";
import { TitleBar } from "./app/components/TitleBar";
import {
  PROVIDER_ORDER,
  PROVIDERS,
  buildProvidersFromStatus,
  getAccount,
  getModel,
  reconcileEffort,
} from "./app/data/providers";
import type {
  AccountUsage,
  AttachmentFile,
  Conversation as PresentationConv,
  ConversationGroup,
  ContextUsage,
  EffortLevel,
  Message as PresentationMessage,
  ProviderId,
  ProviderInfo,
  Selection,
} from "./app/types";
import { useConversation } from "./hooks/useConversation";
import { useProviders } from "./hooks/useProviders";
import { useEvents } from "./hooks/useEvents";
import { deleteConversation } from "./lib/api";
import type { AttachmentInfo, ProviderSessionRecord } from "./lib/types";

const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 420;
const BROWSER_MIN = 320;
const BROWSER_MAX = 720;

function clamp(n: number, min: number, max: number) {
  return Math.min(max, Math.max(min, n));
}

function formatAgo(dateStr: string): string {
  const d = new Date(dateStr);
  if (isNaN(d.getTime())) return "now";
  const diffMs = Math.max(0, Date.now() - d.getTime());
  const diffMins = Math.floor(diffMs / 60000);
  if (diffMins < 1) return "now";
  if (diffMins < 60) return `${diffMins}m`;
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return `${diffHours}h`;
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 7) return `${diffDays}d`;
  return `${Math.floor(diffDays / 7)}w`;
}

function formatTime(dateStr: string): string {
  const d = new Date(dateStr);
  if (isNaN(d.getTime())) {
    return new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function formatSize(bytes: number): string {
  if (bytes > 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export default function App() {
  /* ----------------------------------------------------------- backend hooks --- */
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
    setErrorMessage,
    loadConversations,
    createNewConversation,
    renameConversation,
    archiveConversation,
    selectConversation,
    sendMessage,
    uploadAttachment,
    deleteAttachment,
    fetchUsage,
    interrupt,
    respondToApproval,
    streamingAccount,
    streamingModel,
    handleEvent,
  } = useConversation();

  const { providers: detectedProviders, activeProvider, setActiveProvider, detectProviders } =
    useProviders();

  useEvents(handleEvent);

  useEffect(() => {
    detectProviders();
    loadConversations();
  }, [detectProviders, loadConversations]);

  /* -------------------------------------------------------- providers state --- */
  const [providers, setProviders] = useState<Record<ProviderId, ProviderInfo>>(PROVIDERS);

  // Sync detected providers into provider registry
  useEffect(() => {
    if (detectedProviders.length > 0) {
      setProviders((prev) => buildProvidersFromStatus(detectedProviders, prev));
    }
  }, [detectedProviders]);

  const [selection, setSelection] = useState<Selection>({
    provider: "claude",
    accountId: "claude-work",
    modelId: "sonnet",
    effort: "high",
  });

  // Sync activeProvider when changed externally
  useEffect(() => {
    if (activeProvider && activeProvider !== selection.provider) {
      const pId = activeProvider as ProviderId;
      const account = providers[pId]?.accounts[0];
      const model = account?.models[0];
      setSelection((prev) => ({
        provider: pId,
        accountId: account?.id ?? prev.accountId,
        modelId: model?.id ?? prev.modelId,
        effort: reconcileEffort(pId, prev.effort, providers),
      }));
    }
  }, [activeProvider, providers]);

  const selectModel = useCallback(
    (provider: ProviderId, accountId: string, modelId: string) => {
      setActiveProvider(provider);
      setSelection((prev) => ({
        provider,
        accountId,
        modelId,
        effort: reconcileEffort(provider, prev.effort, providers),
      }));
      if (currentConversationId) {
        fetchUsage(currentConversationId, provider, accountId, modelId);
      }
    },
    [setActiveProvider, providers, currentConversationId, fetchUsage],
  );

  const setEffort = useCallback((effort: EffortLevel) => {
    setSelection((prev) => ({ ...prev, effort }));
  }, []);

  const renameAccount = useCallback(
    (provider: ProviderId, accountId: string, label: string) => {
      setProviders((prev) => ({
        ...prev,
        [provider]: {
          ...prev[provider],
          accounts: prev[provider].accounts.map((a) =>
            a.id === accountId ? { ...a, label } : a,
          ),
        },
      }));
    },
    [],
  );

  /* ------------------------------------------------------ conversations state --- */
  const [pinnedMap, setPinnedMap] = useState<Record<string, boolean>>(() => {
    try {
      return JSON.parse(localStorage.getItem("sr_pinned") || "{}");
    } catch {
      return {};
    }
  });

  const [groupMap, setGroupMap] = useState<Record<string, string>>(() => {
    try {
      return JSON.parse(localStorage.getItem("sr_groups") || "{}");
    } catch {
      return {};
    }
  });

  const [groups, setGroups] = useState<ConversationGroup[]>(() => {
    try {
      const saved = localStorage.getItem("sr_group_list");
      if (saved) return JSON.parse(saved);
    } catch {}
    return [
      { id: "work", label: "Work", collapsed: false },
      { id: "personal", label: "Personal", collapsed: true },
    ];
  });

  const togglePin = useCallback((id: string) => {
    setPinnedMap((prev) => {
      const next = { ...prev, [id]: !prev[id] };
      try {
        localStorage.setItem("sr_pinned", JSON.stringify(next));
      } catch {}
      return next;
    });
  }, []);

  const moveToGroup = useCallback((id: string, groupId: string | undefined) => {
    setGroupMap((prev) => {
      const next = { ...prev };
      if (groupId) {
        next[id] = groupId;
      } else {
        delete next[id];
      }
      try {
        localStorage.setItem("sr_groups", JSON.stringify(next));
      } catch {}
      return next;
    });
  }, []);

  const toggleGroup = useCallback((groupId: string) => {
    setGroups((prev) => {
      const next = prev.map((g) => (g.id === groupId ? { ...g, collapsed: !g.collapsed } : g));
      try {
        localStorage.setItem("sr_group_list", JSON.stringify(next));
      } catch {}
      return next;
    });
  }, []);

  const createGroup = useCallback(() => {
    setGroups((prev) => {
      const next = [...prev, { id: `g${Date.now()}`, label: "New group", collapsed: false }];
      try {
        localStorage.setItem("sr_group_list", JSON.stringify(next));
      } catch {}
      return next;
    });
  }, []);

  const presentationConversations: PresentationConv[] = conversations.map((c) => ({
    id: c.id,
    title: c.title || "New conversation",
    lastProvider: (c.provider as ProviderId) || "claude",
    ago: formatAgo(c.updated_at),
    pinned: !!pinnedMap[c.id],
    groupId: groupMap[c.id],
  }));

  const handleSelectConversation = (id: string) => {
    selectConversation(id, selection.provider, selection.accountId, selection.modelId);
  };

  const handleNewChat = useCallback(async () => {
    await createNewConversation();
  }, [createNewConversation]);

  const handleRenameConversation = async (id: string, title: string) => {
    await renameConversation(id, title);
  };

  const handleArchiveConversation = async (id: string) => {
    try {
      await archiveConversation(id);
    } catch (e) {
      console.error("Archive conversation failed:", e);
      setErrorMessage(String(e));
    }
  };

  const handleDeleteConversation = async (id: string) => {
    try {
      await deleteConversation(id);
      await loadConversations();
      if (currentConversationId === id) {
        await createNewConversation();
      }
    } catch (e) {
      console.error("Delete conversation failed:", e);
      setErrorMessage(String(e));
    }
  };

  /* ------------------------------------------------------------- messages --- */
  const presentationMessages: PresentationMessage[] = messages.map((m) => {
    if (m.role === "user") {
      return {
        id: m.id,
        seq: m.seq,
        role: "user",
        content: m.content,
        timestamp: formatTime(m.created_at),
        attachments: m.attachments?.map((a) => ({
          id: a.id,
          name: a.name,
          size: formatSize(a.size),
          kind: a.mime_type?.startsWith("image/") ? "image" : "file",
        })),
      };
    }

    const pId = (m.provider as ProviderId) || "claude";
    const pInfo = providers[pId];
    return {
      id: m.id,
      seq: m.seq,
      role: "assistant",
      content: m.content,
      timestamp: formatTime(m.created_at),
      origin: {
        provider: pId,
        accountLabel: pInfo?.accounts[0]?.label || "Default",
        modelLabel: m.model || pInfo?.accounts[0]?.models[0]?.label || "Default Model",
        effort: null,
      },
      thinking: m.thinking
        ? {
            content: m.thinking,
            wordCount: m.thinking.trim().split(/\s+/).filter(Boolean).length,
          }
        : undefined,
    };
  });

  // Append in-flight streaming turn
  if (isStreaming) {
    const maxSeq = messages.reduce((n, m) => Math.max(n, m.seq), 0);
    const pId = (streamingProvider as ProviderId) || selection.provider;
    const account = getAccount(selection.provider, selection.accountId, providers);
    const model = getModel(selection.provider, selection.accountId, selection.modelId, providers);

    presentationMessages.push({
      id: "live-turn",
      seq: maxSeq + 1,
      role: "assistant",
      content: streamingContent,
      timestamp: new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
      origin: {
        provider: pId,
        accountLabel: account?.label || "Default",
        modelLabel: model?.label || selection.modelId,
        effort: selection.effort,
      },
      thinking: streamingThinking
        ? {
            content: streamingThinking,
            wordCount: streamingThinking.trim().split(/\s+/).filter(Boolean).length,
          }
        : undefined,
      toolCalls: activeTools.map((t) => ({
        id: t.id,
        name: t.tool_name,
        args:
          typeof t.input === "object" && t.input !== null
            ? t.input
            : { input: String(t.input ?? "") },
        result:
          typeof t.output === "object" && t.output !== null
            ? JSON.stringify(t.output, null, 2)
            : t.output
              ? String(t.output)
              : undefined,
        status: t.status,
      })),
      approval: pendingApproval
        ? {
            tool: pendingApproval.tool_name,
            command: pendingApproval.input?.command || pendingApproval.tool_name,
            target:
              pendingApproval.input?.path ||
              pendingApproval.input?.target ||
              pendingApproval.description,
            note: pendingApproval.description,
          }
        : undefined,
    });
  }

  /* ------------------------------------------------------------- layout --- */
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [sidebarWidth, setSidebarWidth] = useState(264);
  const [browserOpen, setBrowserOpen] = useState(false);
  const [browserWidth, setBrowserWidth] = useState(440);
  const [inspectorOpen, setInspectorOpen] = useState(false);

  const drag = useRef<{ edge: "sidebar" | "browser"; x: number; start: number } | null>(null);

  useEffect(() => {
    const move = (e: MouseEvent) => {
      const d = drag.current;
      if (!d) return;
      if (d.edge === "sidebar") {
        setSidebarWidth(clamp(d.start + (e.clientX - d.x), SIDEBAR_MIN, SIDEBAR_MAX));
      } else {
        setBrowserWidth(clamp(d.start - (e.clientX - d.x), BROWSER_MIN, BROWSER_MAX));
      }
    };
    const up = () => {
      drag.current = null;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", up);
    return () => {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", up);
    };
  }, []);

  const startDrag = (edge: "sidebar" | "browser") => (e: React.MouseEvent) => {
    e.preventDefault();
    drag.current = {
      edge,
      x: e.clientX,
      start: edge === "sidebar" ? sidebarWidth : browserWidth,
    };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };

  /* -------------------------------------------------------- shortcuts --- */
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        handleNewChat();
      }
      if (e.key === "Escape" && isStreaming) {
        interrupt(streamingProvider || activeProvider);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [isStreaming, handleNewChat, interrupt, streamingProvider, activeProvider]);

  /* ------------------------------------------------------------- send --- */
  const uploadedAttachmentsRef = useRef<Map<string, AttachmentInfo>>(new Map());

  const handleSend = async (text: string, atts: AttachmentFile[]) => {
    let targetConvId = currentConversationId;
    if (!targetConvId) {
      const newConv = await createNewConversation();
      if (!newConv) return;
      targetConvId = newConv.id;
    }
    const attInfos = atts
      .map((a) => uploadedAttachmentsRef.current.get(a.id))
      .filter((a): a is AttachmentInfo => a !== undefined);
    await sendMessage(
      text,
      selection.provider,
      attInfos,
      selection.modelId,
      selection.accountId,
      selection.effort ?? undefined,
      targetConvId
    );
  };

  const handleUploadFile = async (file: File): Promise<AttachmentFile> => {
    let targetConvId = currentConversationId;
    if (!targetConvId) {
      const newConv = await createNewConversation();
      if (!newConv) throw new Error("Could not create conversation");
      targetConvId = newConv.id;
    }
    const att = await uploadAttachment(file, targetConvId);
    if (!att) throw new Error("Upload failed");
    uploadedAttachmentsRef.current.set(att.id, att);
    return {
      id: att.id,
      name: att.name,
      size: formatSize(att.size),
      kind: att.mime_type?.startsWith("image/") ? "image" : "file",
    };
  };

  const handleRemoveAttachment = async (id: string) => {
    uploadedAttachmentsRef.current.delete(id);
    try {
      await deleteAttachment(id);
    } catch (e) {
      console.error("Delete attachment error:", e);
    }
  };

  /* ----------------------------------------------------------- context usage --- */
  const model = getModel(selection.provider, selection.accountId, selection.modelId, providers);
  const contextUsage: ContextUsage = {
    used: currentUsage?.context_tokens ?? 0,
    window:
      currentUsage?.context_window ??
      (providers[selection.provider]?.capabilities.contextWindow
        ? model?.contextWindow ?? 200_000
        : null),
    input: currentUsage?.input_tokens ?? null,
    output: currentUsage?.output_tokens ?? null,
    reasoning: currentUsage?.reasoning_tokens ?? null,
    cacheRead: currentUsage?.cache_read_tokens ?? null,
    updatedAt: currentUsage?.created_at ?? null,
  };

  const accountUsage: AccountUsage[] = PROVIDER_ORDER.flatMap((pId) => {
    const p = providers[pId];
    if (!p) return [];
    return p.accounts.map((acc) => ({
      providerId: pId,
      accountId: acc.id,
      windows: p.capabilities.usageLimits
        ? [
            { label: "5-hour", percent: null, resetsAt: null },
            { label: "Weekly", percent: null, resetsAt: null },
          ]
        : null,
    }));
  });

  const maxSeq = messages.reduce((n, m) => Math.max(n, m.seq), 0);
  const getSessionAccount = (s: ProviderSessionRecord): string => {
    if (!s.metadata_json) return "default";
    try {
      const parsed = JSON.parse(s.metadata_json);
      return parsed.account || "default";
    } catch {
      return "default";
    }
  };

  const findSession = (pId: ProviderId) => {
    return (
      providerSessions.find(
        (s) =>
          s.provider === pId &&
          getSessionAccount(s) === selection.accountId &&
          s.model === selection.modelId,
      ) ??
      providerSessions.find(
        (s) =>
          s.provider === pId &&
          getSessionAccount(s) === selection.accountId &&
          !s.model,
      ) ??
      null
    );
  };

  const nativeSessions: Record<ProviderId, string | null> = {
    claude: findSession("claude")?.provider_session_id ?? null,
    codex: findSession("codex")?.provider_session_id ?? null,
    gemini: findSession("gemini")?.provider_session_id ?? null,
  };
  const syncCursors: Record<ProviderId, { synced: number; current: number }> = {
    claude: {
      synced: findSession("claude")?.synced_through_seq ?? 0,
      current: maxSeq,
    },
    codex: {
      synced: findSession("codex")?.synced_through_seq ?? 0,
      current: maxSeq,
    },
    gemini: {
      synced: findSession("gemini")?.synced_through_seq ?? 0,
      current: maxSeq,
    },
  };

  const running = isStreaming
    ? {
        provider: (streamingProvider as ProviderId) || selection.provider,
        label: `${providers[(streamingProvider as ProviderId) || selection.provider]?.label || "AI"} generating`,
      }
    : null;

  return (
    <div className="sr-app">
      <TitleBar
        workspace="~/seralyn-core"
        providers={providers}
        onOpenProvider={() => setInspectorOpen(true)}
        onOpenSettings={() => setInspectorOpen(true)}
      />

      <div className="sr-body">
        {sidebarOpen && (
          <>
            <div className="sr-sidebar" style={{ width: sidebarWidth }}>
              <Sidebar
                conversations={presentationConversations}
                groups={groups}
                activeId={currentConversationId || ""}
                onSelect={handleSelectConversation}
                onNewChat={handleNewChat}
                onRename={handleRenameConversation}
                onTogglePin={togglePin}
                onMoveToGroup={moveToGroup}
                onArchive={handleArchiveConversation}
                onDelete={handleDeleteConversation}
                onToggleGroup={toggleGroup}
                onCreateGroup={createGroup}
                onOpenConnections={() => setInspectorOpen(true)}
              />
            </div>
            <div
              className="sr-resizer"
              onMouseDown={startDrag("sidebar")}
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize sidebar"
            >
              <span />
            </div>
          </>
        )}

        <div className="sr-main">
          <ChatHeader
            title={currentConversation?.title ?? "New conversation"}
            onRename={(title) =>
              currentConversationId && handleRenameConversation(currentConversationId, title)
            }
            sidebarOpen={sidebarOpen}
            onToggleSidebar={() => setSidebarOpen((o) => !o)}
            browserOpen={browserOpen}
            onToggleBrowser={() => setBrowserOpen((o) => !o)}
            inspectorOpen={inspectorOpen}
            onToggleInspector={() => setInspectorOpen((o) => !o)}
            running={running}
            providers={providers}
          />

          <Conversation
            messages={presentationMessages}
            generating={isStreaming}
            onRespondApproval={(approved) =>
              respondToApproval(
                streamingProvider || activeProvider,
                approved,
                streamingAccount || selection.accountId,
                streamingModel || selection.modelId
              )
            }
          />

          <Composer
            providers={providers}
            selection={selection}
            onSelect={selectModel}
            onSetEffort={setEffort}
            onRenameAccount={renameAccount}
            contextUsage={contextUsage}
            accountUsage={accountUsage}
            generating={isStreaming}
            onSend={handleSend}
            onStop={() =>
              interrupt(
                streamingProvider || activeProvider,
                streamingAccount || selection.accountId,
                streamingModel || selection.modelId
              )
            }
            webOpen={browserOpen}
            onToggleWeb={() => setBrowserOpen((o) => !o)}
            onManageProviders={() => setInspectorOpen(true)}
            onViewUsage={() => setInspectorOpen(true)}
            onUploadFile={handleUploadFile}
            onRemoveAttachment={handleRemoveAttachment}
            syncedThroughTurn={maxSeq}
          />
        </div>

        {browserOpen && (
          <>
            <div
              className="sr-resizer"
              onMouseDown={startDrag("browser")}
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize browser panel"
            >
              <span />
            </div>
            <div className="sr-browser" style={{ width: browserWidth }}>
              <BrowserPanel url="http://localhost:5173" onClose={() => setBrowserOpen(false)} />
            </div>
          </>
        )}

        {inspectorOpen && (
          <Inspector
            messages={presentationMessages}
            onClose={() => setInspectorOpen(false)}
            nativeSessions={nativeSessions}
            syncCursors={syncCursors}
            providers={providers}
          />
        )}
      </div>
    </div>
  );
}
