import { useCallback, useEffect, useState } from "react";
import { api, inTauri, on } from "@/lib/ipc";
import type { Settings } from "@/types/ipc";
import { useTauriEvent } from "./useTauriEvent";

export interface SettingsApi {
  settings: Settings | null;
  saving: boolean;
  warnings: string[];
  error: string | null;
  update: (patch: Partial<Settings>) => Promise<void>;
  dismissWarnings: () => void;
}

/** Loads settings and persists changes through the backend, which validates them. */
export function useSettings(): SettingsApi {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (inTauri()) api.getSettings().then(setSettings, (e) => setError(String(e)));
  }, []);
  useTauriEvent(on.settingsChanged, setSettings);

  const update = useCallback(
    async (patch: Partial<Settings>) => {
      if (!settings) return;
      setSaving(true);
      setError(null);
      try {
        const result = await api.updateSettings({ ...settings, ...patch });
        setSettings(result.settings);
        setWarnings(result.warnings);
      } catch (e) {
        setError(String(e));
      } finally {
        setSaving(false);
      }
    },
    [settings],
  );

  return {
    settings,
    saving,
    warnings,
    error,
    update,
    dismissWarnings: () => setWarnings([]),
  };
}
