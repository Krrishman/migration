// Accessible, shadcn-style primitives built on native elements (and Radix
// Dialog for focus-trapped modals). All are keyboard operable.

import * as RD from "@radix-ui/react-dialog";
import clsx from "clsx";
import { Check, Copy, Loader2, X } from "lucide-react";
import { forwardRef, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger" | "outline";

export const Button = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" | "lg"; busy?: boolean }>(
  function Button({ variant = "secondary", size = "md", busy, className, children, disabled, ...rest }, ref) {
    return (
      <button
        ref={ref}
        className={clsx(
          "inline-flex items-center justify-center gap-2 rounded-lg font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
          size === "sm" && "h-8 px-3 text-[13px]",
          size === "md" && "h-9 px-4",
          size === "lg" && "h-11 px-5 text-[15px]",
          variant === "primary" && "bg-accent text-accent-fg hover:bg-accent/90",
          variant === "secondary" && "bg-surface-2 text-fg hover:bg-border/70",
          variant === "outline" && "border border-border bg-surface text-fg hover:bg-surface-2",
          variant === "ghost" && "text-fg hover:bg-surface-2",
          variant === "danger" && "bg-danger text-white hover:bg-danger/90",
          className,
        )}
        disabled={disabled || busy}
        aria-busy={busy || undefined}
        {...rest}
      >
        {busy && <Loader2 className="h-4 w-4 animate-spin" aria-hidden />}
        {children}
      </button>
    );
  },
);

export function Card({ className, children, ...rest }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={clsx("rounded-xl border border-border bg-surface shadow-sm", className)} {...rest}>
      {children}
    </div>
  );
}

export type Tone = "neutral" | "accent" | "ok" | "warn" | "danger" | "teal";

export function Badge({ tone = "neutral", children, className, title }: { tone?: Tone; children: ReactNode; className?: string; title?: string }) {
  return (
    <span
      title={title}
      className={clsx(
        "inline-flex items-center gap-1 whitespace-nowrap rounded-full px-2 py-0.5 text-[12px] font-medium",
        tone === "neutral" && "bg-surface-2 text-muted",
        tone === "accent" && "bg-accent/10 text-accent",
        tone === "teal" && "bg-teal/10 text-teal",
        tone === "ok" && "bg-ok/10 text-ok",
        tone === "warn" && "bg-warn/15 text-warn",
        tone === "danger" && "bg-danger/10 text-danger",
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Checkbox({ label, className, ...rest }: InputHTMLAttributes<HTMLInputElement> & { label: string }) {
  return <input type="checkbox" aria-label={label} className={clsx("h-4 w-4 shrink-0 cursor-pointer rounded border-border accent-[rgb(var(--accent))] disabled:cursor-not-allowed", className)} {...rest} />;
}

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function Input({ className, ...rest }, ref) {
  return (
    <input
      ref={ref}
      className={clsx("h-9 w-full rounded-lg border border-border bg-surface px-3 text-fg placeholder:text-muted/70 focus:border-accent focus:outline-none", className)}
      {...rest}
    />
  );
});

export function Select({ className, children, ...rest }: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select className={clsx("h-9 rounded-lg border border-border bg-surface px-2 text-fg focus:border-accent focus:outline-none", className)} {...rest}>
      {children}
    </select>
  );
}

export function Progress({ value, label, tone = "accent", className }: { value: number | null; label: string; tone?: "accent" | "ok" | "warn" | "danger"; className?: string }) {
  const indeterminate = value === null;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(value)}
      className={clsx("relative h-2 w-full overflow-hidden rounded-full bg-surface-2", className)}
    >
      <div
        className={clsx(
          "h-full rounded-full transition-[width] duration-300",
          tone === "accent" && "bg-accent",
          tone === "ok" && "bg-ok",
          tone === "warn" && "bg-warn",
          tone === "danger" && "bg-danger",
          indeterminate && "w-1/3 animate-pulse",
        )}
        style={indeterminate ? undefined : { width: `${value}%` }}
      />
    </div>
  );
}

export function Alert({ tone = "neutral", title, children, icon }: { tone?: Tone; title?: string; children?: ReactNode; icon?: ReactNode }) {
  return (
    <div
      role={tone === "danger" ? "alert" : "status"}
      className={clsx(
        "flex gap-3 rounded-lg border px-4 py-3",
        tone === "neutral" && "border-border bg-surface-2",
        tone === "accent" && "border-accent/30 bg-accent/5",
        tone === "teal" && "border-teal/30 bg-teal/5",
        tone === "ok" && "border-ok/30 bg-ok/5",
        tone === "warn" && "border-warn/40 bg-warn/5",
        tone === "danger" && "border-danger/40 bg-danger/5",
      )}
    >
      {icon && <div className="mt-0.5 shrink-0">{icon}</div>}
      <div className="min-w-0 space-y-1">
        {title && <p className="font-semibold">{title}</p>}
        {children && <div className="text-[13px] text-fg/90">{children}</div>}
      </div>
    </div>
  );
}

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  wide,
  dismissible = true,
}: {
  dismissible?: boolean;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  title: string;
  description?: string;
  children?: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  return (
    <RD.Root open={open} onOpenChange={onOpenChange}>
      <RD.Portal>
        <RD.Overlay className="fixed inset-0 z-40 bg-black/40" />
        <RD.Content
          onEscapeKeyDown={dismissible ? undefined : (e) => e.preventDefault()}
          onPointerDownOutside={dismissible ? undefined : (e) => e.preventDefault()}
          className={clsx(
            "fixed left-1/2 top-1/2 z-50 max-h-[88vh] w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-xl border border-border bg-surface p-6 shadow-xl focus:outline-none",
            wide ? "max-w-3xl" : "max-w-lg",
          )}
        >
          <div className="mb-4 flex items-start justify-between gap-4">
            <div>
              <RD.Title className="text-lg font-semibold">{title}</RD.Title>
              {description ? <RD.Description className="mt-1 text-[13px] text-muted">{description}</RD.Description> : <RD.Description className="sr-only">{title}</RD.Description>}
            </div>
            {dismissible && (
              <RD.Close asChild>
                <Button variant="ghost" size="sm" aria-label="Close">
                  <X className="h-4 w-4" />
                </Button>
              </RD.Close>
            )}
          </div>
          <div className="space-y-3">{children}</div>
          {footer && <div className="mt-6 flex flex-wrap justify-end gap-2">{footer}</div>}
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}

export function CopyButton({ text, label = "Copy path" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted hover:bg-surface-2 hover:text-fg"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setDone(true);
          setTimeout(() => setDone(false), 1200);
        } catch {
          /* clipboard unavailable */
        }
      }}
    >
      {done ? <Check className="h-3.5 w-3.5 text-ok" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  );
}

export function KeyValue({ label, value, mono }: { label: string; value: ReactNode; mono?: boolean }) {
  return (
    <div className="min-w-0">
      <dt className="text-[12px] text-muted">{label}</dt>
      <dd className={clsx("truncate font-medium", mono && "font-mono text-[12px]")}>{value}</dd>
    </div>
  );
}

export function Spinner({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center gap-2 text-muted" role="status">
      <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
      {label}
    </span>
  );
}

export function SectionTitle({ children, actions }: { children: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-3 flex items-center justify-between gap-3">
      <h2 className="text-[15px] font-semibold">{children}</h2>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  );
}
