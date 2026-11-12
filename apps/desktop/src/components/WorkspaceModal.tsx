import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
export function WorkspaceModal({
  title,
  busy,
  onClose,
  children,
  className,
}: {
  title: string;
  busy: boolean;
  onClose: () => void;
  children: React.ReactNode;
  className?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const { t } = useTranslation();
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    ref.current?.showModal();
    ref.current
      ?.querySelector<HTMLElement>(
        "[data-initial-focus], input, select, textarea",
      )
      ?.focus();
    return () => {
      ref.current?.close();
      const target = before?.isConnected
        ? before
        : before?.id
          ? document.getElementById(before.id)
          : null;
      target?.focus();
    };
  }, []);
  return (
    <dialog
      ref={ref}
      className={className}
      aria-labelledby="work-dialog-title"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <div className="modal-title">
        <h2 id="work-dialog-title">{title}</h2>
        <button
          type="button"
          aria-label={t("work.close")}
          disabled={busy}
          onClick={onClose}
        >
          <X aria-hidden="true" />
        </button>
      </div>
      {children}
    </dialog>
  );
}
