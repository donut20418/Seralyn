import { Check } from "lucide-react";
import { EFFORT_LABEL } from "../data/providers";
import type { EffortLevel } from "../types";

interface Props {
  options: EffortLevel[];
  value: EffortLevel | null;
  providerLabel: string;
  modelLabel: string;
  onPick: (level: EffortLevel) => void;
}

export function EffortMenu({ options, value, providerLabel, modelLabel, onPick }: Props) {
  return (
    <div className="sr-pop" style={{ left: 250, width: 216 }} role="dialog" aria-label="Reasoning effort">
      <div className="sr-pop__head">
        <span className="sr-label">Reasoning effort</span>
      </div>
      <div style={{ padding: 5 }}>
        {options.map((level) => (
          <button key={level} className="sr-menu-item" onClick={() => onPick(level)} aria-current={value === level}>
            <span className="sr-spacer" style={{ flexGrow: 1 }}>
              {EFFORT_LABEL[level]}
            </span>
            {value === level && <Check size={12} color="var(--sr-accent)" />}
          </button>
        ))}
      </div>
      <div className="sr-pop__foot" style={{ height: "auto", padding: "8px 12px", lineHeight: 1.5 }}>
        Levels offered by {providerLabel} for {modelLabel}.
      </div>
    </div>
  );
}
