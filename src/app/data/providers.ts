import type {
  ConnectionStatus,
  EffortLevel,
  ModelInfo,
  ProviderId,
  ProviderInfo,
  Selection,
} from "../types";

/**
 * Capability registry.
 *
 * In the shipped app this is populated by the provider core from what each
 * official CLI actually reports. The UI reads capabilities from here and from
 * nowhere else, so an unsupported control is never rendered. Nothing in the UI
 * may infer, estimate or invent a capability that is absent.
 */
export const PROVIDERS: Record<ProviderId, ProviderInfo> = {
  claude: {
    id: "claude",
    label: "Claude",
    color: "var(--sr-claude)",
    cliVersion: "2.1.4",
    status: "connected",
    capabilities: {
      effort: ["low", "medium", "high", "xhigh"],
      usageLimits: true,
      contextWindow: true,
    },
    accounts: [
      {
        id: "claude-work",
        label: "Work",
        status: "connected",
        models: [
          { id: "claude-3-7-sonnet-latest", label: "Claude 3.7 Sonnet", contextWindow: 200_000 },
          { id: "claude-3-5-sonnet-latest", label: "Claude 3.5 Sonnet", contextWindow: 200_000 },
          { id: "claude-3-5-haiku-latest", label: "Claude 3.5 Haiku", contextWindow: 200_000 },
          { id: "claude-3-opus-latest", label: "Claude 3 Opus", contextWindow: 200_000 },
        ],
      },
      {
        id: "claude-personal",
        label: "Personal",
        status: "connected",
        models: [
          { id: "claude-3-7-sonnet-latest", label: "Claude 3.7 Sonnet", contextWindow: 200_000 },
          { id: "claude-3-5-sonnet-latest", label: "Claude 3.5 Sonnet", contextWindow: 200_000 },
        ],
      },
    ],
  },
  codex: {
    id: "codex",
    label: "Codex",
    color: "var(--sr-codex)",
    cliVersion: "0.48.0",
    status: "connected",
    capabilities: {
      // This CLI exposes three levels — "Extra High" is never offered.
      effort: ["low", "medium", "high"],
      usageLimits: true,
      contextWindow: true,
    },
    accounts: [
      {
        id: "codex-main",
        label: "Main",
        status: "connected",
        models: [
          { id: "o3-mini", label: "o3-mini", contextWindow: 200_000 },
          { id: "o1", label: "o1", contextWindow: 200_000 },
          { id: "gpt-4o", label: "GPT-4o", contextWindow: 128_000 },
          { id: "gpt-4.5-preview", label: "GPT-4.5 Preview", contextWindow: 128_000 },
        ],
      },
    ],
  },
  gemini: {
    id: "gemini",
    label: "Gemini",
    color: "var(--sr-gemini)",
    cliVersion: "0.9.2",
    status: "connected",
    capabilities: {
      // No reasoning control reported -> the Effort chip is not rendered.
      effort: null,
      // No plan/quota endpoint -> usage reads "Unavailable", never a guess.
      usageLimits: false,
      contextWindow: true,
    },
    accounts: [
      {
        id: "gemini-studio",
        label: "Studio",
        status: "connected",
        models: [
          { id: "gemini-2.5-flash", label: "Gemini 2.5 Flash", contextWindow: 1_000_000 },
          { id: "gemini-2.5-pro", label: "Gemini 2.5 Pro", contextWindow: 1_000_000 },
          { id: "gemini-2.0-flash", label: "Gemini 2.0 Flash", contextWindow: 1_000_000 },
        ],
      },
    ],
  },
};

import type { ProviderStatus } from "../../lib/types";

export const PROVIDER_ORDER: ProviderId[] = ["claude", "codex", "gemini"];

export const EFFORT_LABEL: Record<EffortLevel, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
};

export function getProvider(
  id: ProviderId,
  providers: Record<ProviderId, ProviderInfo> = PROVIDERS,
): ProviderInfo {
  return providers[id] ?? PROVIDERS[id];
}

export function getAccount(
  providerId: ProviderId,
  accountId: string,
  providers: Record<ProviderId, ProviderInfo> = PROVIDERS,
) {
  return (providers[providerId] ?? PROVIDERS[providerId])?.accounts.find((a) => a.id === accountId);
}

export function getModel(
  providerId: ProviderId,
  accountId: string,
  modelId: string,
  providers: Record<ProviderId, ProviderInfo> = PROVIDERS,
): ModelInfo | undefined {
  return getAccount(providerId, accountId, providers)?.models.find((m) => m.id === modelId);
}

/** Effort levels the selected provider actually exposes, or null. */
export function effortOptions(
  providerId: ProviderId,
  providers: Record<ProviderId, ProviderInfo> = PROVIDERS,
): EffortLevel[] | null {
  return (providers[providerId] ?? PROVIDERS[providerId])?.capabilities.effort ?? null;
}

/**
 * Reconcile an effort level against a newly selected provider.
 * Downgrades a level the new provider does not offer and drops it entirely
 * when the provider exposes no reasoning control.
 */
export function reconcileEffort(
  providerId: ProviderId,
  current: EffortLevel | null,
  providers: Record<ProviderId, ProviderInfo> = PROVIDERS,
): EffortLevel | null {
  const options = effortOptions(providerId, providers);
  if (!options) return null;
  if (current && options.includes(current)) return current;
  return options.includes("medium") ? "medium" : options[options.length - 1];
}

export function describeSelection(
  selection: Selection,
  providers: Record<ProviderId, ProviderInfo> = PROVIDERS,
): string {
  const provider = getProvider(selection.provider, providers);
  const account = getAccount(selection.provider, selection.accountId, providers);
  const model = getModel(selection.provider, selection.accountId, selection.modelId, providers);
  return [provider?.label, account?.label, model?.label].filter(Boolean).join(" · ");
}

export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n % 1_000 === 0 ? 0 : 1)}K`;
  return `${n}`;
}

export function buildProvidersFromStatus(
  statuses: ProviderStatus[],
  existing: Record<ProviderId, ProviderInfo> = PROVIDERS,
): Record<ProviderId, ProviderInfo> {
  const result = { ...existing };
  for (const s of statuses) {
    const pId = s.provider as ProviderId;
    if (!result[pId]) continue;

    let status: ConnectionStatus = "connected";
    if (!s.installed) {
      status = "not-installed";
    } else if (s.authenticated === "NotAuthenticated") {
      status = "signin-required";
    } else if (s.error) {
      status = "error";
    }

    result[pId] = {
      ...result[pId],
      cliVersion: s.version ?? result[pId].cliVersion,
      status,
      capabilities: {
        effort: s.capabilities.reasoning
          ? (pId === "codex" ? ["low", "medium", "high"] : ["low", "medium", "high", "xhigh"])
          : null,
        usageLimits: s.capabilities.token_usage,
        contextWindow: s.capabilities.context_window,
      },
    };
  }
  return result;
}
