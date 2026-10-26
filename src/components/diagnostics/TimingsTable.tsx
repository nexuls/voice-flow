import type { SessionRecord } from "@/types/ipc";

type TimingKey = Exclude<keyof NonNullable<SessionRecord["timings"]>, "session">;

const COLUMNS: [TimingKey, string][] = [
  ["captureOpenMs", "Mic open"],
  ["overlayVisibleMs", "Overlay"],
  ["recordedMs", "Recorded"],
  ["transcribedMs", "Speech"],
  ["finalizeMs", "Finalize"],
  ["inferenceMs", "Inference"],
  ["insertMs", "Insert"],
  ["releaseToInsertMs", "Release → text"],
];

const fmt = (v: number | null) =>
  v === null ? "—" : v >= 1000 ? `${(v / 1000).toFixed(2)} s` : `${Math.round(v)} ms`;

/** Latency of recent dictations. Contains timings only, never audio or text. */
export function TimingsTable({ rows }: { rows: SessionRecord[] }) {
  if (rows.length === 0) {
    return <p className="text-sm text-muted-foreground">No dictations yet in this session.</p>;
  }
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-right font-mono text-xs tabular-nums">
        <caption className="sr-only">Latency per dictation</caption>
        <thead className="text-muted-foreground">
          <tr>
            <th scope="col" className="py-1 text-left font-normal">
              #
            </th>
            <th scope="col" className="px-2 py-1 text-left font-normal">
              Outcome
            </th>
            {COLUMNS.map(([, label]) => (
              <th key={label} scope="col" className="px-2 py-1 font-normal">
                {label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.session} className="border-t">
              <td className="py-1 text-left">{r.session}</td>
              <td className="px-2 py-1 text-left">{r.outcome ?? "—"}</td>
              {COLUMNS.map(([key]) => (
                <td key={key} className="px-2 py-1">
                  {fmt(r.timings?.[key] ?? null)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
