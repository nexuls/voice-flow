import { Mic, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Waveform } from "@/components/waveform/Waveform";
import { pushLevel } from "@/hooks/useAudioLevels";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { api, inTauri, on } from "@/lib/ipc";
import type { InputDevice, MicTestResult } from "@/types/ipc";
import { Row } from "./Section";

const DEFAULT = "__default__";
const TEST_SECONDS = 3;

interface Props {
  value: string | null;
  disabled?: boolean;
  onChange: (deviceId: string | null) => void;
}

export function MicrophoneRows({ value, disabled, onChange }: Props) {
  const [devices, setDevices] = useState<InputDevice[]>([]);
  const [testing, setTesting] = useState(false);
  const [levels, setLevels] = useState<number[]>([]);
  const [result, setResult] = useState<MicTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!inTauri()) return;
    api.listMicrophones().then(setDevices, (e) => setError(String(e)));
  }, []);
  useEffect(() => refresh(), [refresh]);

  useTauriEvent(
    on.micTestLevel,
    useCallback((l: { level: number }) => setLevels((prev) => pushLevel(prev, l.level, 48)), []),
  );

  const test = async () => {
    setTesting(true);
    setResult(null);
    setError(null);
    setLevels([]);
    try {
      setResult(await api.testMicrophone(TEST_SECONDS));
    } catch (e) {
      setError(String(e));
    } finally {
      setTesting(false);
    }
  };

  const missing = value !== null && devices.length > 0 && !devices.some((d) => d.id === value);

  return (
    <>
      <Row
        label="Microphone"
        description={
          missing ? "The selected microphone is not connected; the default is used." : undefined
        }
        control={(id) => (
          <div className="flex items-center gap-2">
            <Select
              value={value ?? DEFAULT}
              disabled={disabled}
              onValueChange={(v) => onChange(v === DEFAULT ? null : v)}
            >
              <SelectTrigger id={id} className="w-64">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={DEFAULT}>System default</SelectItem>
                {devices.map((d) => (
                  <SelectItem key={d.id} value={d.id}>
                    {d.name}
                    {d.isDefault ? " (default)" : ""}
                  </SelectItem>
                ))}
                {missing && value && <SelectItem value={value}>Disconnected device</SelectItem>}
              </SelectContent>
            </Select>
            <Button variant="ghost" size="icon" onClick={refresh} aria-label="Refresh microphones">
              <RefreshCw />
            </Button>
          </div>
        )}
      />
      <Row
        label="Test microphone"
        description={`Records ${TEST_SECONDS} seconds and shows the level. Nothing is saved or transcribed.`}
      >
        <div className="flex items-center gap-3">
          <div className="rounded-md bg-muted/60 px-2 py-1 text-foreground">
            <Waveform levels={levels} bars={32} className="[&>span]:bg-foreground/70" />
          </div>
          <Button variant="outline" size="sm" onClick={test} disabled={testing || disabled}>
            <Mic />
            {testing ? "Listening…" : "Test"}
          </Button>
        </div>
      </Row>
      {(result || error) && (
        <div className="px-4 py-3 text-sm" aria-live="polite">
          {error && (
            <p role="alert" className="text-destructive">
              {error}
            </p>
          )}
          {result && (
            <p className={result.signalDetected ? "" : "text-destructive"}>
              {result.signalDetected
                ? "Microphone works."
                : "Very little sound was detected. Check that the microphone is not muted."}{" "}
              <span className="text-muted-foreground">
                {result.deviceName} · {result.sampleRate / 1000} kHz · peak{" "}
                {Math.round(result.peak * 100)}%
              </span>
            </p>
          )}
        </div>
      )}
    </>
  );
}
