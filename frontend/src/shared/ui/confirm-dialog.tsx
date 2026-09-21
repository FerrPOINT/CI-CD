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
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  return (
    <AlertDialog
      open={open}
      onOpenChange={(v) => {
        if (!v && !pending) onCancel();
      }}
    >
      <AlertDialogContent className="min-w-0 w-[calc(100vw-2rem)] max-h-[calc(100dvh-2rem)] overflow-y-auto">
        <AlertDialogHeader className="min-w-0 text-left">
          <AlertDialogTitle className="min-w-0 break-all">{title}</AlertDialogTitle>
          {description && (
            <AlertDialogDescription className="break-words">{description}</AlertDialogDescription>
          )}
          {error && <p role="alert" className="break-words text-sm text-danger">{error}</p>}
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel className="min-h-10" disabled={pending} onClick={onCancel}>
            {cancelLabel ?? t("common.cancel")}
          </AlertDialogCancel>
          <AlertDialogAction
            className="min-h-10 text-danger-foreground hover:opacity-100"
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
