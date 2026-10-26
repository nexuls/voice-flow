import { Download, Loader2, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { api, inTauri, on } from "@/lib/ipc";
import type { ModelInfo, ModelProgress } from "@/types/ipc";
import { Row, Section } from "./Section";

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export function modelLabel(info: ModelInfo): { text: string; tone: "ok" | "warn" | "muted" } {
  if (info.installing) return { text: "Installing", tone: "muted" };
  switch (info.status.state) {
    case "installed":
      return info.loaded ? { text: "Ready", tone: "ok" } : { text: "Loading", tone: "muted" };
    case "notInstalled":
      return { text: "Not installed", tone: "warn" };
    case "incomplete":
      return { text: "Incomplete download", tone: "warn" };
    case "invalid":
      return { text: "Damaged", tone: "warn" };
  }
}

export function ModelSection() {
  const [info, setInfo] = useState<ModelInfo | null>(null);
  const [progress, setProgress] = useState<ModelProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (inTauri()) api.modelInfo().then(setInfo);
  }, []);
  useTauriEvent(on.modelStatus, setInfo);
  useTauriEvent(on.modelProgress, setProgress);

  const install = useCallback(async () => {
    setError(null);
    setProgress({ downloaded: 0, total: info?.size ?? 1 });
    try {
      setInfo(await api.installModel());
    } catch (e) {
      setError(String(e));
      setInfo(await api.modelInfo());
    } finally {
      setProgress(null);
    }
  }, [info?.size]);

  const remove = useCallback(async () => {
    setError(null);
    try {
      setInfo(await api.removeModel());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  if (!info) return null;
  const label = modelLabel(info);
  const installed = info.status.state === "installed";
  const pct = progress ? Math.round((progress.downloaded / progress.total) * 100) : 0;

  return (
    <Section
      title="Speech model"
      description="Recognition runs entirely on this computer. The model is downloaded once."
    >
      <Row
        label={info.name}
        description={
          <>
            {formatBytes(info.size)} · {info.license} ·{" "}
            <span className="break-all">huggingface.co/Cactus-Compute/whistle</span>
          </>
        }
      >
        <Badge
          variant={
            label.tone === "ok" ? "default" : label.tone === "warn" ? "destructive" : "secondary"
          }
        >
          {label.text}
        </Badge>
        {info.installing || progress ? (
          <Button variant="outline" size="sm" onClick={() => api.cancelModelInstall()}>
            Cancel
          </Button>
        ) : installed ? (
          <Button variant="ghost" size="sm" onClick={remove} aria-label="Remove speech model">
            <Trash2 />
            Remove
          </Button>
        ) : (
          <Button size="sm" onClick={install}>
            <Download />
            {info.status.state === "notInstalled" ? "Install" : "Reinstall"}
          </Button>
        )}
      </Row>
      {progress && (
        <div className="space-y-1 px-4 py-3" aria-live="polite">
          <Progress value={pct} aria-label="Download progress" />
          <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin motion-reduce:animate-none" />
            {formatBytes(progress.downloaded)} of {formatBytes(progress.total)} — verifying checksum
            on completion
          </p>
        </div>
      )}
      {error && (
        <p role="alert" className="px-4 py-3 text-sm text-destructive">
          {error}
        </p>
      )}
    </Section>
  );
}
