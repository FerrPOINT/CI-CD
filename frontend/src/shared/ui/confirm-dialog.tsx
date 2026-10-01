import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@sdlc/ui/ui";
import { useTranslation } from "react-i18next";
import type { RefObject } from "react";

interface ConfirmDialogProps {
  open: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  title: string;
  description?: string;
  error?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  closeOnConfirm?: boolean;
  pending?: boolean;
  confirmVariant?: "default" | "destructive";
  returnFocusRef?: RefObject<HTMLElement | null>;
}

export function ConfirmDialog({
  open,
  onConfirm,
  onCancel,
  title,
  description,
  error,
  confirmLabel,
  cancelLabel,
  closeOnConfirm = true,
  pending = false,
  confirmVariant = "destructive",
  returnFocusRef,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  return (
    <AlertDialog
      open={open}
      onOpenChange={(v) => {
        if (!v && !pending) onCancel();
      }}
    >
      <AlertDialogContent
        className="min-w-0 w-[calc(100vw-2rem)] max-h-[calc(100dvh-2rem)] overflow-y-auto"
        onCloseAutoFocus={(event) => {
          if (returnFocusRef?.current?.isConnected) {
            event.preventDefault();
            returnFocusRef.current.focus();
          }
        }}
      >
        <AlertDialogHeader className="min-w-0 text-left">
          <AlertDialogTitle className="min-w-0 break-all">{title}</AlertDialogTitle>
          {description && (
            <AlertDialogDescription className="[overflow-wrap:anywhere]">{description}</AlertDialogDescription>
          )}
          {error && <p role="alert" className="break-words text-sm text-danger">{error}</p>}
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel className="min-h-10 sm:min-h-10" disabled={pending} onClick={onCancel}>
            {cancelLabel ?? t("common.cancel")}
          </AlertDialogCancel>
          <AlertDialogAction
            className={confirmVariant === "default" ? "min-h-10 sm:min-h-10 bg-accent text-accent-foreground hover:bg-accent-hover" : "min-h-10 sm:min-h-10 hover:opacity-100"}
            disabled={pending}
            onClick={(event) => {
              if (!closeOnConfirm) event.preventDefault();
              onConfirm();
            }}
          >
            {confirmLabel ?? t("common.delete")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
