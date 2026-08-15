import { useCallback, useEffect, useRef, useState } from "preact/hooks";

interface Props {
  text: string;
  label?: string;
}

export function CopyButton({ text, label = "copy" }: Props) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const onCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text);
      setState("copied");
    } catch {
      setState("failed");
    }
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setState("idle"), 1400);
  }, [text]);

  const shown = state === "idle" ? label : state;

  return (
    <button
      type="button"
      class={state === "copied" ? "copy-button is-copied" : "copy-button"}
      onClick={onCopy}
      aria-label={`Copy ${label}`}
    >
      <span aria-live="polite">{shown}</span>
    </button>
  );
}
