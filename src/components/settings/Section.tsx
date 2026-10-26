import { type ReactNode, useId } from "react";
import { cn } from "@/lib/utils";

export function Section({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="space-y-1">
      <div className="px-1 pb-1">
        <h2 id={id} className="text-sm font-semibold tracking-tight">
          {title}
        </h2>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      <div className="divide-y divide-border rounded-xl border bg-card">{children}</div>
    </section>
  );
}

/**
 * A labelled settings row. `control` receives the id to put on the input so
 * the label is associated with it.
 */
export function Row({
  label,
  description,
  control,
  children,
  className,
}: {
  label: string;
  description?: ReactNode;
  control?: (id: string) => ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  const id = useId();
  return (
    <div className={cn("flex flex-wrap items-center gap-x-6 gap-y-2 px-4 py-3", className)}>
      <div className="min-w-48 flex-1">
        <label htmlFor={id} className="text-sm font-medium">
          {label}
        </label>
        {description && <div className="text-xs text-muted-foreground">{description}</div>}
      </div>
      {control?.(id)}
      {children}
    </div>
  );
}
